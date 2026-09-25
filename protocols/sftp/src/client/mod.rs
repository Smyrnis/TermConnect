use std::{
    fmt,
    fs::DirBuilder,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use russh::{
    Signer,
    client::{self, AuthResult, Handle},
    keys,
    keys::{
        HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate,
        agent::{AgentIdentity, client::AgentClient},
    },
};
use tokio::sync::{mpsc, oneshot};

const SFTP_REQUEST_TIMEOUT_SECS: u64 = 60;
const SFTP_MAX_CONCURRENT_READS: usize = 8;
pub const SFTP_MAX_CONCURRENT_WRITES: usize = 16;
pub const SFTP_MAX_WRITE_PACKET_LEN: u32 = 32 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum HostKeyStatus {
    Known,
    Unknown,
    Changed { line: usize },
}

pub fn host_key_status(known_hosts: &Path, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus> {
    match keys::check_known_hosts_path(host, port, key, known_hosts) {
        Ok(true) => Ok(HostKeyStatus::Known),
        Ok(false) => Ok(HostKeyStatus::Unknown),
        Err(keys::Error::KeyChanged { line }) => {
            Ok(HostKeyStatus::Changed { line: line_in_file(known_hosts, line).unwrap_or(line) })
        }
        Err(err) => Err(err.into()),
    }
}

fn line_in_file(known_hosts: &Path, uncommented_line: usize) -> Option<usize> {
    let contents = std::fs::read_to_string(known_hosts).ok()?;
    contents
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.starts_with('#'))
        .nth(uncommented_line.checked_sub(1)?)
        .map(|(index, _)| index + 1)
}

pub fn trust_host_key(known_hosts: &Path, host: &str, port: u16, key: &PublicKey) -> Result<()> {
    if let Some(ssh_dir) = known_hosts.parent() {
        DirBuilder::new().recursive(true).mode(0o700).create(ssh_dir)?;
    }
    keys::known_hosts::learn_known_hosts_path(host, port, key, known_hosts)?;
    Ok(())
}

pub fn default_known_hosts() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join(".ssh").join("known_hosts"))
}

#[derive(Debug)]
pub struct HostKeyDeclined;

impl fmt::Display for HostKeyDeclined {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the host key was not trusted")
    }
}

impl std::error::Error for HostKeyDeclined {}

pub struct HostKeyQuestion {
    pub key_type: String,
    pub fingerprint: String,
    pub reply: oneshot::Sender<bool>,
}

pub struct HostKeyCheck {
    host: String,
    port: u16,
    known_hosts: Option<PathBuf>,
    questions: mpsc::Sender<HostKeyQuestion>,
}

impl HostKeyCheck {
    pub fn new(host: &str, port: u16, known_hosts: Option<PathBuf>, questions: mpsc::Sender<HostKeyQuestion>) -> Self {
        Self { host: host.to_string(), port, known_hosts, questions }
    }

    pub async fn verify(&self, key: &PublicKey) -> Result<bool> {
        let Some(known_hosts) = self.known_hosts.as_deref() else {
            return Err(anyhow!(
                "can't verify the host key for {}:{} \u{2014} no home directory to find ~/.ssh/known_hosts in",
                self.host,
                self.port
            ));
        };

        match host_key_status(known_hosts, &self.host, self.port, key)? {
            HostKeyStatus::Known => Ok(true),
            HostKeyStatus::Changed { line } => Err(anyhow!(
                "REMOTE HOST IDENTIFICATION HAS CHANGED for {}:{} (see {} line {}) \u{2014} refusing to connect",
                self.host,
                self.port,
                known_hosts.display(),
                line
            )),
            HostKeyStatus::Unknown => {
                if !self.ask_to_trust(key).await {
                    return Err(HostKeyDeclined.into());
                }
                if let Err(err) = trust_host_key(known_hosts, &self.host, self.port, key) {
                    tracing::warn!("could not save the host key for {}:{}: {err:#}", self.host, self.port);
                }
                Ok(true)
            }
        }
    }

    async fn ask_to_trust(&self, key: &PublicKey) -> bool {
        let (reply, answer) = oneshot::channel();
        let question = HostKeyQuestion {
            key_type: key.algorithm().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
            reply,
        };
        if self.questions.send(question).await.is_err() {
            return false;
        }
        answer.await.unwrap_or(false)
    }
}

pub struct PorthmosHandler {
    host_key: HostKeyCheck,
}

impl client::Handler for PorthmosHandler {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            return Err(anyhow!(
                "the host key for {} is a certificate, which Porthmos does not yet support",
                self.host_key.host
            ));
        };
        self.host_key.verify(key).await
    }
}

pub async fn open_sftp(handle: &Handle<PorthmosHandler>) -> Result<russh_sftp::client::SftpSession> {
    let channel = handle.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    let config = russh_sftp::client::Config {
        request_timeout_secs: SFTP_REQUEST_TIMEOUT_SECS,
        max_concurrent_reads: SFTP_MAX_CONCURRENT_READS,
        max_concurrent_writes: SFTP_MAX_CONCURRENT_WRITES,
        max_write_packet_len: SFTP_MAX_WRITE_PACKET_LEN,
        ..russh_sftp::client::Config::default()
    };
    let sftp = russh_sftp::client::SftpSession::new_with_config(channel.into_stream(), config).await?;
    Ok(sftp)
}

pub async fn connect(host_key: HostKeyCheck) -> Result<Handle<PorthmosHandler>> {
    let config = Arc::new(client::Config::default());
    let address = (host_key.host.clone(), host_key.port);

    client::connect(config, address, PorthmosHandler { host_key }).await
}

pub async fn authenticate_non_interactive(
    handle: &mut Handle<PorthmosHandler>, username: &str, identity_file: Option<&Path>, password: Option<&str>,
) -> Result<bool> {
    if authenticate_with_agent(handle, username).await? {
        return Ok(true);
    }

    if let Some(identity_file) = identity_file
        && authenticate_with_key_file(handle, username, identity_file).await?
    {
        return Ok(true);
    }

    if let Some(password) = password
        && authenticate_password(handle, username, password).await?
    {
        return Ok(true);
    }

    Ok(false)
}

async fn authenticate_with_agent(handle: &mut Handle<PorthmosHandler>, username: &str) -> Result<bool> {
    let Ok(mut agent) = AgentClient::connect_env().await else {
        return Ok(false);
    };

    let Ok(identities) = agent.request_identities().await else {
        return Ok(false);
    };

    for identity in identities {
        let public_key = identity.public_key().into_owned();
        let mut signer = AgentSigner(&mut agent);

        let result = handle.authenticate_publickey_with(username, public_key, None, &mut signer).await;

        if let Ok(AuthResult::Success) = result {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn authenticate_with_key_file(
    handle: &mut Handle<PorthmosHandler>, username: &str, identity_file: &Path,
) -> Result<bool> {
    let Ok(private_key) = PrivateKey::read_openssh_file(identity_file) else {
        return Ok(false);
    };

    if private_key.is_encrypted() {
        return Ok(false);
    }

    let key = PrivateKeyWithHashAlg::new(Arc::new(private_key), None);

    match handle.authenticate_publickey(username, key).await? {
        AuthResult::Success => Ok(true),
        AuthResult::Failure { .. } => Ok(false),
    }
}

pub async fn authenticate_password(
    handle: &mut Handle<PorthmosHandler>, username: &str, password: &str,
) -> Result<bool> {
    match handle.authenticate_password(username, password).await? {
        AuthResult::Success => Ok(true),
        AuthResult::Failure { .. } => Ok(false),
    }
}

struct AgentSigner<'a>(&'a mut AgentClient<tokio::net::UnixStream>);

impl Signer for AgentSigner<'_> {
    type Error = anyhow::Error;

    async fn auth_sign(
        &mut self, key: &AgentIdentity, hash_alg: Option<HashAlg>, to_sign: Vec<u8>,
    ) -> Result<Vec<u8>, Self::Error> {
        let public_key = key.public_key().into_owned();
        let signature = self.0.sign_request_signature(&public_key, hash_alg, &to_sign).await?;
        Ok(signature.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests;
