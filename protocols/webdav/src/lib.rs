#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod auth;
mod client;
mod errors;
mod fs;
mod idle;
mod paths;
mod propfind;
mod settings;
mod upload;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, PoisonError},
    time::Duration,
};

use anyhow::anyhow;
use porthmos_tls::{KnownCertificates, ProblemSlot, TrustProblem, ask_to_trust, client_config};
use porthmos_vfs::{
    Answer, Choice, ConnectionForm, ErrorKind, FileSystem, OptionField, OptionKind, Prompter, Protocol, ProtocolError,
    Question, Target, async_trait,
};
use reqwest::{Method, Response, StatusCode};

use crate::{
    auth::strongest,
    client::{DavClient, challenges_of},
    errors::{Failure, redirect_error},
    fs::{WebDavFs, propfind},
    paths::Locator,
    settings::WebDavSettings,
};

const DEFAULT_PORT: u16 = 443;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const SECURITY_CHOICES: &[Choice] =
    &[Choice { value: "https", label: "HTTPS" }, Choice { value: "http", label: "HTTP (unencrypted)" }];

pub struct WebDav {
    known_certificates: PathBuf,
    timeout: Duration,
}

impl WebDav {
    pub fn new(known_certificates: PathBuf) -> Self {
        Self { known_certificates, timeout: DEFAULT_TIMEOUT }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl Protocol for WebDav {
    fn id(&self) -> &'static str {
        "webdav"
    }

    fn display_name(&self) -> &'static str {
        "WebDAV"
    }

    fn default_port(&self) -> u16 {
        DEFAULT_PORT
    }

    fn connection_form(&self) -> ConnectionForm {
        let mut form = ConnectionForm::standard(DEFAULT_PORT);
        form.username.required = false;
        form.options = vec![
            OptionField {
                key: "security",
                label: "Security",
                required: false,
                kind: OptionKind::Choice { choices: SECURITY_CHOICES, default: "https" },
            },
            OptionField { key: "root", label: "Root path", required: false, kind: OptionKind::Text { default: "/" } },
        ];
        form
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        let settings = WebDavSettings::from_target(target);
        let store = KnownCertificates::new(self.known_certificates.clone());
        let (tls, problem) = if settings.secure {
            let (config, problem) = client_config(&settings.host, settings.port, store.clone());
            (Some(config), Some(problem))
        } else {
            (None, None)
        };
        let locator = Locator::new(&settings.origin(), &settings.root);
        let client = DavClient::new(locator, tls, self.timeout).map_err(|err| connect_failed(err.into()))?;
        if let (false, Some(saved)) = (settings.username.is_empty(), &settings.password) {
            client.use_credentials(&settings.username, saved);
        }
        let first = probe_trusted(&client, target, &settings, &store, problem.as_ref(), prompter).await?;
        let answered = authenticate(&client, first, target, &settings, prompter).await?;
        check_probe(&answered, &settings)?;
        let partial_update = supports_partial_update(&client).await;
        Ok(Arc::new(WebDavFs::new(Arc::new(client), partial_update)))
    }
}

fn connect_failed(failure: Failure) -> ProtocolError {
    failure.into_error(ErrorKind::Connect)
}

fn cancelled() -> ProtocolError {
    ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled"))
}

fn take_problem(slot: &ProblemSlot) -> Option<TrustProblem> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

async fn probe(client: &DavClient) -> Result<Response, Failure> {
    propfind(client, Path::new("/"), true, "0").await
}

async fn probe_trusted(
    client: &DavClient, target: &Target, settings: &WebDavSettings, store: &KnownCertificates,
    problem: Option<&ProblemSlot>, prompter: &mut dyn Prompter,
) -> Result<Response, ProtocolError> {
    loop {
        if let Some(slot) = problem {
            take_problem(slot);
        }
        match probe(client).await {
            Ok(response) => return Ok(response),
            Err(err) => match problem.and_then(take_problem) {
                Some(found) => {
                    ask_to_trust(found, &target.name, &settings.host, settings.port, store, prompter).await?
                }
                None => return Err(connect_failed(err)),
            },
        }
    }
}

async fn authenticate(
    client: &DavClient, first: Response, target: &Target, settings: &WebDavSettings, prompter: &mut dyn Prompter,
) -> Result<Response, ProtocolError> {
    if first.status() != StatusCode::UNAUTHORIZED {
        return Ok(first);
    }
    let scheme = strongest(challenges_of(&first)).map_err(|names| {
        ProtocolError::new(
            ErrorKind::Auth,
            anyhow!("the server requires an unsupported authentication scheme: {}", names.join(", ")),
        )
    })?;
    if settings.username.is_empty() {
        return Err(ProtocolError::new(ErrorKind::AuthRejected, anyhow!("the server requires a username")));
    }
    client.use_scheme(scheme);
    let question = Question::Password { username: settings.username.clone(), name: target.name.clone() };
    let Some(Answer::Password(password)) = prompter.ask(question).await else {
        return Err(cancelled());
    };
    client.use_credentials(&settings.username, &password);
    let response = probe(client).await.map_err(connect_failed)?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(ProtocolError::new(ErrorKind::AuthRejected, anyhow!("Authentication failed for {}", target.name)));
    }
    Ok(response)
}

fn check_probe(response: &Response, settings: &WebDavSettings) -> Result<(), ProtocolError> {
    let status = response.status();
    if status == StatusCode::MULTI_STATUS {
        return Ok(());
    }
    if status.is_redirection() {
        return Err(redirect_error(response.headers()));
    }
    if status == StatusCode::NOT_FOUND {
        return Err(ProtocolError::new(ErrorKind::Connect, anyhow!("Root path {} not found", settings.root)));
    }
    Err(ProtocolError::new(
        ErrorKind::Connect,
        anyhow!("{} is not a WebDAV server (PROPFIND returned {status})", settings.host),
    ))
}

async fn supports_partial_update(client: &DavClient) -> bool {
    let root = client.locator().href(Path::new("/"), true);
    let Ok(response) = client.send(Method::OPTIONS, &root, |builder| builder).await else {
        return false;
    };
    response
        .headers()
        .get_all("dav")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.to_ascii_lowercase().contains("sabredav-partialupdate"))
}

#[cfg(test)]
mod tests;
