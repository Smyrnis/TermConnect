#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod client;
mod fs;
mod search;
mod shell;
pub mod ssh_config;

use std::{collections::BTreeMap, future::Future, path::Path, sync::Arc};

use anyhow::anyhow;
use porthmos_vfs::{
    Answer, Environment, ErrorKind, FileSystem, Prompter, Protocol, ProtocolError, Question, ShellInvocation, Target,
    async_trait,
};
use russh::client::Handle;
use tokio::sync::mpsc;

use crate::{
    client::{HostKeyCheck, HostKeyDeclined, HostKeyQuestion, PorthmosHandler},
    fs::SftpFs,
};

const DEFAULT_PORT: u16 = 22;
const FALLBACK_USERNAME: &str = "root";

pub struct Sftp;

fn discovered_target(host: ssh_config::SshConfigHost, env: &Environment) -> Target {
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
}

fn matches_its_ssh_config_alias(target: &Target, env: &Environment) -> bool {
    let Ok(hosts) = ssh_config::load(env.home.as_deref()) else {
        return false;
    };
    hosts.into_iter().filter(|host| host.name == target.name).map(|host| discovered_target(host, env)).any(
        |configured| {
            configured.host == target.host
                && configured.port == target.port
                && configured.username == target.username
                && configured.option("identity_file") == target.option("identity_file")
        },
    )
}

fn connect_error(err: anyhow::Error) -> ProtocolError {
    if err.downcast_ref::<HostKeyDeclined>().is_some() {
        return ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled"));
    }
    ProtocolError::new(ErrorKind::Connect, err)
}

async fn answer_host_key_questions<T>(
    connecting: impl Future<Output = T>, asked: &mut mpsc::Receiver<HostKeyQuestion>, prompter: &mut dyn Prompter,
    target: &Target,
) -> T {
    tokio::pin!(connecting);
    loop {
        tokio::select! {
            result = &mut connecting => return result,
            Some(question) = asked.recv() => {
                let HostKeyQuestion { key_type, fingerprint, reply } = question;
                let trust = Question::TrustHostKey {
                    name: target.name.clone(),
                    host: target.host.clone(),
                    port: target.port,
                    key_type,
                    fingerprint,
                };
                let trusted = matches!(prompter.ask(trust).await, Some(Answer::Confirmed));
                let _ = reply.send(trusted);
            }
        }
    }
}

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
        Ok(hosts.into_iter().map(|host| discovered_target(host, env)).collect())
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        let (questions, mut asked) = mpsc::channel(1);
        let host_key = HostKeyCheck::new(&target.host, target.port, client::default_known_hosts(), questions);
        let mut handle = answer_host_key_questions(client::connect(host_key), &mut asked, prompter, target)
            .await
            .map_err(connect_error)?;

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
        let alias = matches_its_ssh_config_alias(target, env).then_some(target.name.as_str());
        Some(shell::invocation_for(target, alias, sshpass))
    }
}

#[cfg(test)]
mod tests;
