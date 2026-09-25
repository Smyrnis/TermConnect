#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod certificates;
mod errors;
mod fs;
mod listing;
mod pool;
mod session;
mod settings;
mod streams;

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::anyhow;
use porthmos_vfs::{
    Answer, Choice, ConnectionForm, ErrorKind, FileSystem, OptionField, OptionKind, Prompter, Protocol, ProtocolError,
    Question, Target, async_trait,
};

use crate::{
    certificates::{KnownCertificates, TrustProblem, client_config},
    fs::FtpFs,
    pool::Pool,
    session::{Connection, DEFAULT_TIMEOUT, Login, OpenError, SessionContext, bounded, login, open, prepare},
    settings::FtpSettings,
};

const DEFAULT_PORT: u16 = 21;
const SECURITY_CHOICES: &[Choice] = &[
    Choice { value: "plain", label: "Plain FTP (unencrypted)" },
    Choice { value: "explicit", label: "Explicit TLS (FTPS)" },
    Choice { value: "implicit", label: "Implicit TLS (FTPS)" },
];

pub struct Ftp {
    known_certificates: PathBuf,
    timeout: Duration,
}

impl Ftp {
    pub fn new(known_certificates: PathBuf) -> Self {
        Self { known_certificates, timeout: DEFAULT_TIMEOUT }
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl Protocol for Ftp {
    fn id(&self) -> &'static str {
        "ftp"
    }

    fn display_name(&self) -> &'static str {
        "FTP"
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
                kind: OptionKind::Choice { choices: SECURITY_CHOICES, default: "explicit" },
            },
            OptionField {
                key: "passive",
                label: "Passive mode",
                required: false,
                kind: OptionKind::Toggle { default: true },
            },
        ];
        form
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        let settings = FtpSettings::from_target(target);
        let store = KnownCertificates::new(self.known_certificates.clone());
        let (context, mut connection) = open_trusted(target, &settings, &store, self.timeout, prompter).await?;
        let password = authenticate(&mut connection, target, &settings, self.timeout, prompter).await?;
        bounded(self.timeout, prepare(&mut connection, &settings)).await?;
        let pool = Pool::new(context, password, connection);
        Ok(Arc::new(FtpFs::new(pool).await?))
    }
}

fn cancelled() -> ProtocolError {
    ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled"))
}

async fn open_trusted(
    target: &Target, settings: &FtpSettings, store: &KnownCertificates, timeout: Duration, prompter: &mut dyn Prompter,
) -> Result<(SessionContext, Connection), ProtocolError> {
    loop {
        let (tls, problem) = client_config(&settings.host, settings.port, store.clone());
        let context = SessionContext { settings: settings.clone(), tls, problem, timeout };
        match open(&context).await {
            Ok(connection) => return Ok((context, connection)),
            Err(OpenError::Trust(TrustProblem::Unknown(details))) => {
                let question = Question::TrustCertificate {
                    name: target.name.clone(),
                    host: settings.host.clone(),
                    port: settings.port,
                    fingerprint: details.fingerprint.clone(),
                    subject: details.subject.clone(),
                    expires: details.expires.clone(),
                };
                if !matches!(prompter.ask(question).await, Some(Answer::Confirmed)) {
                    return Err(cancelled());
                }
                store.remember(&settings.host, settings.port, &details)?;
            }
            Err(OpenError::Trust(TrustProblem::Changed)) => {
                return Err(ProtocolError::new(
                    ErrorKind::Connect,
                    anyhow!(
                        "CERTIFICATE CHANGED for {}:{} \u{2014} refusing to connect (see {})",
                        settings.host,
                        settings.port,
                        store.path().display()
                    ),
                ));
            }
            Err(OpenError::Trust(TrustProblem::Store(message))) => {
                return Err(ProtocolError::new(ErrorKind::Connect, anyhow!(message)));
            }
            Err(OpenError::Other(err)) => return Err(err),
        }
    }
}

fn requires_encryption(reply: &str) -> bool {
    let reply = reply.to_ascii_lowercase();
    ["encrypt", "tls", "ssl", "secure"].iter().any(|hint| reply.contains(hint))
}

async fn authenticate(
    connection: &mut Connection, target: &Target, settings: &FtpSettings, timeout: Duration,
    prompter: &mut dyn Prompter,
) -> Result<String, ProtocolError> {
    let rejected = || ProtocolError::new(ErrorKind::AuthRejected, anyhow!("Authentication failed for {}", target.name));
    if let Some(saved) = settings.password.clone() {
        let reply = match bounded(timeout, login(connection, &settings.username, &saved)).await? {
            Login::Accepted => return Ok(saved),
            Login::Rejected(reply) => reply,
        };
        if requires_encryption(&reply) {
            return Err(ProtocolError::new(ErrorKind::Auth, anyhow!("{reply}")));
        }
        if settings.is_anonymous() {
            return Err(rejected());
        }
    }
    let question = Question::Password { username: settings.username.clone(), name: target.name.clone() };
    let Some(Answer::Password(password)) = prompter.ask(question).await else {
        return Err(cancelled());
    };
    match bounded(timeout, login(connection, &settings.username, &password)).await? {
        Login::Accepted => Ok(password),
        Login::Rejected(reply) if requires_encryption(&reply) => {
            Err(ProtocolError::new(ErrorKind::Auth, anyhow!("{reply}")))
        }
        Login::Rejected(_) => Err(rejected()),
    }
}

#[cfg(test)]
mod tests;
