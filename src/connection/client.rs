use std::{path::Path, sync::Arc};

use anyhow::{Result, anyhow};
use russh::{
    Signer,
    client::{self, AuthResult, Handle},
    keys,
    keys::{
        HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate,
        agent::{AgentIdentity, client::AgentClient},
    },
};

use super::ConnectionEntry;

const SFTP_REQUEST_TIMEOUT_SECS: u64 = 60;
const SFTP_MAX_CONCURRENT_READS: usize = 8;

pub struct TermConnectHandler {
    host: String,
    port: u16,
}

impl client::Handler for TermConnectHandler {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            return Err(anyhow!("the host key for {} is a certificate, which TermConnect does not yet support", self.host));
        };

        match keys::check_known_hosts(&self.host, self.port, key) {
            Ok(true) => Ok(true),
            Ok(false) => Err(anyhow!("host key for {}:{} is not in ~/.ssh/known_hosts \u{2014} connect once with the system `ssh` client to trust it, or add it manually", self.host, self.port)),
            Err(keys::Error::KeyChanged { line }) => Err(anyhow!("REMOTE HOST IDENTIFICATION HAS CHANGED for {}:{} (see ~/.ssh/known_hosts line {}) \u{2014} refusing to connect", self.host, self.port, line)),
            Err(err) => Err(err.into()),
        }
    }
}

pub async fn open_sftp(handle: &Handle<TermConnectHandler>) -> Result<russh_sftp::client::SftpSession> {
    let channel = handle.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    let config = russh_sftp::client::Config { request_timeout_secs: SFTP_REQUEST_TIMEOUT_SECS, max_concurrent_reads: SFTP_MAX_CONCURRENT_READS, ..russh_sftp::client::Config::default() };
    let sftp = russh_sftp::client::SftpSession::new_with_config(channel.into_stream(), config).await?;
    Ok(sftp)
}

pub async fn connect(host: &str, port: u16) -> Result<Handle<TermConnectHandler>> {
    let config = Arc::new(client::Config::default());
    let handler = TermConnectHandler { host: host.to_string(), port };

    client::connect(config, (host, port), handler).await
}

pub async fn authenticate_non_interactive(handle: &mut Handle<TermConnectHandler>, entry: &ConnectionEntry) -> Result<bool> {
    if authenticate_with_agent(handle, &entry.username).await? {
        return Ok(true);
    }

    if let Some(identity_file) = &entry.identity_file
        && authenticate_with_key_file(handle, &entry.username, identity_file).await?
    {
        return Ok(true);
    }

    if let Some(password) = &entry.password
        && authenticate_password(handle, &entry.username, password).await?
    {
        return Ok(true);
    }

    Ok(false)
}

async fn authenticate_with_agent(handle: &mut Handle<TermConnectHandler>, username: &str) -> Result<bool> {
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

async fn authenticate_with_key_file(handle: &mut Handle<TermConnectHandler>, username: &str, identity_file: &Path) -> Result<bool> {
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

pub async fn authenticate_password(handle: &mut Handle<TermConnectHandler>, username: &str, password: &str) -> Result<bool> {
    match handle.authenticate_password(username, password).await? {
        AuthResult::Success => Ok(true),
        AuthResult::Failure { .. } => Ok(false),
    }
}

struct AgentSigner<'a>(&'a mut AgentClient<tokio::net::UnixStream>);

impl Signer for AgentSigner<'_> {
    type Error = anyhow::Error;

    async fn auth_sign(&mut self, key: &AgentIdentity, hash_alg: Option<HashAlg>, to_sign: Vec<u8>) -> Result<Vec<u8>, Self::Error> {
        let public_key = key.public_key().into_owned();
        let signature = self.0.sign_request_signature(&public_key, hash_alg, &to_sign).await?;
        Ok(signature.as_bytes().to_vec())
    }
}
