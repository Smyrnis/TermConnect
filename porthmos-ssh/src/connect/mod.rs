use std::{future::Future, path::PathBuf};

use anyhow::anyhow;
use porthmos_vfs::{Answer, ErrorKind, Prompter, ProtocolError, Question, Target};
use russh::client::Handle;
use tokio::sync::mpsc;

use crate::{
    client::{self, HostKeyCheck, HostKeyDeclined, HostKeyQuestion, PorthmosHandler},
    discovery::identity_path,
};

#[derive(Debug, Clone)]
pub struct ConnectOptions {
    pub known_hosts: Option<PathBuf>,
    pub use_agent: bool,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self { known_hosts: None, use_agent: true }
    }
}

pub struct Session {
    handle: Handle<PorthmosHandler>,
}

impl Session {
    pub fn handle(&self) -> &Handle<PorthmosHandler> {
        &self.handle
    }
}

pub(crate) fn connect_error(err: anyhow::Error) -> ProtocolError {
    if err.downcast_ref::<HostKeyDeclined>().is_some() {
        return ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled"));
    }
    ProtocolError::new(ErrorKind::Connect, err)
}

pub(crate) async fn answer_host_key_questions<T>(
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

pub async fn connect(
    target: &Target, prompter: &mut dyn Prompter, options: &ConnectOptions,
) -> Result<Session, ProtocolError> {
    let (questions, mut asked) = mpsc::channel(1);
    let known_hosts = options.known_hosts.clone().or_else(client::default_known_hosts);
    let host_key = HostKeyCheck::new(&target.host, target.port, known_hosts, questions);
    let mut handle = answer_host_key_questions(client::connect(host_key), &mut asked, prompter, target)
        .await
        .map_err(connect_error)?;

    let home = std::env::home_dir();
    let identity_file = target.option("identity_file").map(|path| identity_path(path, home.as_deref()));
    match client::authenticate_non_interactive(
        &mut handle,
        &target.username,
        identity_file.as_deref(),
        target.password.as_deref(),
        options.use_agent,
    )
    .await
    {
        Ok(true) => return Ok(Session { handle }),
        Ok(false) => {}
        Err(err) => return Err(ProtocolError::new(ErrorKind::Auth, err)),
    }

    let question = Question::Password { username: target.username.clone(), name: target.name.clone() };
    let Some(Answer::Password(password)) = prompter.ask(question).await else {
        return Err(ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled")));
    };

    match client::authenticate_password(&mut handle, &target.username, &password).await {
        Ok(true) => Ok(Session { handle }),
        Ok(false) => {
            Err(ProtocolError::new(ErrorKind::AuthRejected, anyhow!("Authentication failed for {}", target.name)))
        }
        Err(err) => Err(ProtocolError::new(ErrorKind::Auth, err)),
    }
}

#[cfg(test)]
mod tests;
