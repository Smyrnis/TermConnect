#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod client;
mod fs;
mod search;
mod shell;
pub mod ssh_config;

use std::{collections::BTreeMap, path::Path, sync::Arc};

use anyhow::anyhow;
use porthmos_vfs::{
    Answer, Environment, ErrorKind, FileSystem, Prompter, Protocol, ProtocolError, Question, ShellInvocation, Target,
    async_trait,
};
use russh::client::Handle;

use crate::{client::PorthmosHandler, fs::SftpFs};

const DEFAULT_PORT: u16 = 22;
const FALLBACK_USERNAME: &str = "root";

pub struct Sftp;

async fn start_sftp(handle: Handle<PorthmosHandler>) -> Result<Arc<dyn FileSystem>, ProtocolError> {
    let sftp = client::open_sftp(&handle).await.map_err(|err| ProtocolError::new(ErrorKind::SessionStart, err))?;
    Ok(Arc::new(SftpFs::new(Arc::new(handle), Arc::new(sftp))))
}

#[async_trait]
impl Protocol for Sftp {
    fn id(&self) -> &'static str {
        "sftp"
    }

    fn display_name(&self) -> &'static str {
        "SFTP"
    }

    fn default_port(&self) -> u16 {
        DEFAULT_PORT
    }

    fn discover(&self, env: &Environment) -> Result<Vec<Target>, ProtocolError> {
        let hosts = ssh_config::load(env.home.as_deref())?;
        Ok(hosts
            .into_iter()
            .map(|host| {
                let mut options = BTreeMap::new();
                if let Some(identity_file) = host.identity_file {
                    options.insert("identity_file".to_string(), identity_file.to_string_lossy().into_owned());
                }
                Target {
                    name: host.name,
                    host: host.host_name.unwrap_or_default(),
                    port: host.port.unwrap_or(DEFAULT_PORT),
                    username: host.user.or_else(|| env.user.clone()).unwrap_or_else(|| FALLBACK_USERNAME.to_string()),
                    password: None,
                    options,
                }
            })
            .collect())
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        let mut handle = client::connect(&target.host, target.port)
            .await
            .map_err(|err| ProtocolError::new(ErrorKind::Connect, err))?;

        let identity_file = target.option("identity_file").map(Path::new);
        match client::authenticate_non_interactive(
            &mut handle,
            &target.username,
            identity_file,
            target.password.as_deref(),
        )
        .await
        {
            Ok(true) => return start_sftp(handle).await,
            Ok(false) => {}
            Err(err) => return Err(ProtocolError::new(ErrorKind::Auth, err)),
        }

        let question = Question::Password { username: target.username.clone(), name: target.name.clone() };
        let Some(Answer::Password(password)) = prompter.ask(question).await else {
            return Err(ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled")));
        };

        match client::authenticate_password(&mut handle, &target.username, &password).await {
            Ok(true) => start_sftp(handle).await,
            Ok(false) => {
                Err(ProtocolError::new(ErrorKind::AuthRejected, anyhow!("Authentication failed for {}", target.name)))
            }
            Err(err) => Err(ProtocolError::new(ErrorKind::Auth, err)),
        }
    }

    fn shell_command(&self, target: &Target, env: &Environment) -> Option<ShellInvocation> {
        let sshpass = env.path.as_deref().and_then(shell::find_sshpass_in);
        Some(shell::invocation_for(target, sshpass))
    }
}

#[cfg(test)]
mod tests;
