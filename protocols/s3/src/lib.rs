#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod client;
mod clock;
mod errors;
mod fs;
mod idle;
mod locate;
mod settings;
mod sign;
mod staging;
mod upload;
mod xml;

use std::{
    path::PathBuf,
    sync::{Arc, PoisonError},
    time::Duration,
};

use anyhow::anyhow;
use porthmos_tls::{KnownCertificates, ProblemSlot, TrustProblem, ask_to_trust, client_config};
use porthmos_vfs::{
    Answer, Choice, ConnectionForm, ErrorKind, FileSystem, OptionField, OptionKind, Prompter, Protocol, ProtocolError,
    Question, Target, async_trait,
};
use reqwest::{Method, Response, StatusCode, header::LOCATION};

use crate::{
    client::{Call, S3Client},
    errors::{Failure, S3Error},
    fs::S3Fs,
    settings::S3Settings,
};

const DEFAULT_PORT: u16 = 443;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const SECURITY_CHOICES: &[Choice] =
    &[Choice { value: "https", label: "HTTPS" }, Choice { value: "http", label: "HTTP (unencrypted)" }];
const ADDRESSING_CHOICES: &[Choice] = &[
    Choice { value: "auto", label: "Automatic" },
    Choice { value: "path", label: "Path-style (host/bucket)" },
    Choice { value: "virtual", label: "Virtual-hosted (bucket.host)" },
];
const REJECTED_CREDENTIALS: [&str; 2] = ["InvalidAccessKeyId", "SignatureDoesNotMatch"];

pub struct S3 {
    known_certificates: PathBuf,
    timeout: Duration,
}

impl S3 {
    pub fn new(known_certificates: PathBuf) -> Self {
        Self { known_certificates, timeout: DEFAULT_TIMEOUT }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl Protocol for S3 {
    fn id(&self) -> &'static str {
        "s3"
    }

    fn display_name(&self) -> &'static str {
        "S3"
    }

    fn default_port(&self) -> u16 {
        DEFAULT_PORT
    }

    fn connection_form(&self) -> ConnectionForm {
        let mut form = ConnectionForm::standard(DEFAULT_PORT);
        form.host.label = "Endpoint";
        form.username.label = "Access key ID";
        form.password.label = "Secret access key";
        form.options = vec![
            OptionField {
                key: "security",
                label: "Security",
                required: false,
                kind: OptionKind::Choice { choices: SECURITY_CHOICES, default: "https" },
            },
            OptionField {
                key: "region",
                label: "Region",
                required: false,
                kind: OptionKind::Text { default: "us-east-1" },
            },
            OptionField { key: "bucket", label: "Bucket", required: false, kind: OptionKind::Text { default: "" } },
            OptionField {
                key: "addressing",
                label: "Addressing",
                required: false,
                kind: OptionKind::Choice { choices: ADDRESSING_CHOICES, default: "auto" },
            },
        ];
        form
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        let settings = S3Settings::from_target(target);
        let store = KnownCertificates::new(self.known_certificates.clone());
        let (tls, problem) = if settings.secure {
            let (config, problem) = client_config(&settings.endpoint, settings.port, store.clone());
            (Some(config), Some(problem))
        } else {
            (None, None)
        };
        let client = S3Client::new(&settings, tls, self.timeout).map_err(|err| connect_failed(err.into()))?;
        let mut asked = false;
        if settings.secret.is_none() {
            ask_secret(&client, target, &settings, prompter).await?;
            asked = true;
        }
        let mut region_corrected = false;
        loop {
            let response = probe_trusted(&client, target, &settings, &store, problem.as_ref(), prompter).await?;
            if response.status().is_success() {
                break;
            }
            let status = response.status();
            let location = response.headers().get(LOCATION).and_then(|value| value.to_str().ok()).map(str::to_string);
            let error = client.error(response).await;
            let region_hint = error.region.clone().filter(|region| *region != client.region());
            if let Some(region) = region_hint
                .filter(|_| !region_corrected && (status.is_redirection() || status == StatusCode::BAD_REQUEST))
            {
                client.set_region(&region);
                region_corrected = true;
                continue;
            }
            if status == StatusCode::FORBIDDEN && REJECTED_CREDENTIALS.contains(&error.code.as_str()) {
                if asked {
                    return Err(ProtocolError::new(
                        ErrorKind::AuthRejected,
                        anyhow!("Authentication failed for {}", target.name),
                    ));
                }
                ask_secret(&client, target, &settings, prompter).await?;
                asked = true;
                continue;
            }
            return Err(probe_failure(&error, status, location, &settings));
        }
        Ok(Arc::new(S3Fs::new(Arc::new(client), settings.bucket.clone())))
    }
}

fn connect_failed(failure: Failure) -> ProtocolError {
    failure.into_error(ErrorKind::Connect)
}

fn cancelled() -> ProtocolError {
    ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled"))
}

async fn ask_secret(
    client: &S3Client, target: &Target, settings: &S3Settings, prompter: &mut dyn Prompter,
) -> Result<(), ProtocolError> {
    let question = Question::Password { username: settings.access_key.clone(), name: target.name.clone() };
    let Some(Answer::Password(secret)) = prompter.ask(question).await else {
        return Err(cancelled());
    };
    client.set_secret(&secret);
    Ok(())
}

fn take_problem(slot: &ProblemSlot) -> Option<TrustProblem> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

async fn probe(client: &S3Client, settings: &S3Settings) -> Result<Response, Failure> {
    let call = match &settings.bucket {
        Some(bucket) => Call::new(Method::GET, Some(bucket), "").query("list-type", "2").query("max-keys", "1"),
        None => Call::new(Method::GET, None, ""),
    };
    client.send(call).await
}

async fn probe_trusted(
    client: &S3Client, target: &Target, settings: &S3Settings, store: &KnownCertificates,
    problem: Option<&ProblemSlot>, prompter: &mut dyn Prompter,
) -> Result<Response, ProtocolError> {
    loop {
        if let Some(slot) = problem {
            take_problem(slot);
        }
        match probe(client, settings).await {
            Ok(response) => return Ok(response),
            Err(failure) => match problem.and_then(take_problem) {
                Some(found) => {
                    ask_to_trust(found, &target.name, &settings.endpoint, settings.port, store, prompter).await?
                }
                None => return Err(connect_failed(failure)),
            },
        }
    }
}

fn probe_failure(
    error: &S3Error, status: StatusCode, location: Option<String>, settings: &S3Settings,
) -> ProtocolError {
    let connect = |message: String| ProtocolError::new(ErrorKind::Connect, anyhow!(message));
    match (error.code.as_str(), &settings.bucket) {
        ("NoSuchBucket", Some(bucket)) => connect(format!("Bucket {bucket} not found")),
        ("AccessDenied", None) => connect("This key can't list buckets \u{2014} enter a Bucket name".to_string()),
        ("AccessDenied", Some(bucket)) => connect(format!("Access to bucket {bucket} is denied")),
        ("RequestTimeTooSkewed", _) => {
            connect("the server's clock differs from this computer's by more than 15 minutes".to_string())
        }
        ("", _) if status.is_redirection() => connect(format!(
            "the server redirected to {} \u{2014} check Security, Endpoint and Addressing",
            location.unwrap_or_else(|| "another address".to_string())
        )),
        ("", _) => connect(format!("{} is not an S3 endpoint ({status})", settings.endpoint)),
        _ => connect(error.describe()),
    }
}

#[cfg(test)]
mod tests;
