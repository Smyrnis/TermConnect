use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use russh::client::{self, AuthResult, Handle};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{Signer, keys};

use super::ConnectionEntry;

pub struct TermConnectHandler {
    host: String,
    port: u16,
}

impl client::Handler for TermConnectHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            return Err(anyhow!(
                "the host key for {} is a certificate, which TermConnect does not yet support",
                self.host
            ));
        };

        match keys::check_known_hosts(&self.host, self.port, key) {
            Ok(true) => Ok(true),
            Ok(false) => Err(anyhow!(
                "host key for {}:{} is not in ~/.ssh/known_hosts \u{2014} connect once with the system `ssh` client to trust it, or add it manually",
                self.host,
                self.port
            )),
            Err(keys::Error::KeyChanged { line }) => Err(anyhow!(
                "REMOTE HOST IDENTIFICATION HAS CHANGED for {}:{} (see ~/.ssh/known_hosts line {}) \u{2014} refusing to connect",
                self.host,
                self.port,
                line
            )),
            Err(err) => Err(err.into()),
        }
    }
}

/// Opens the SFTP subsystem on an already-authenticated SSH session.
pub async fn open_sftp(
    handle: &Handle<TermConnectHandler>,
) -> Result<russh_sftp::client::SftpSession> {
    let channel = handle.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    let sftp = russh_sftp::client::SftpSession::new(channel.into_stream()).await?;
    Ok(sftp)
}

/// Opens a TCP connection and completes the SSH handshake, including host
/// key verification against `~/.ssh/known_hosts`. Does not authenticate.
pub async fn connect(host: &str, port: u16) -> Result<Handle<TermConnectHandler>> {
    let config = Arc::new(client::Config::default());
    let handler = TermConnectHandler {
        host: host.to_string(),
        port,
    };

    client::connect(config, (host, port), handler).await
}

/// Tries non-interactive authentication methods in the roadmap's priority
/// order: an `ssh-agent`, then the entry's identity file. Returns `true` if
/// authentication succeeded, `false` if neither method was available or
/// accepted (in which case the caller should fall back to a password
/// prompt).
pub async fn authenticate_non_interactive(
    handle: &mut Handle<TermConnectHandler>,
    entry: &ConnectionEntry,
) -> Result<bool> {
    if authenticate_with_agent(handle, &entry.username).await? {
        return Ok(true);
    }

    if let Some(identity_file) = &entry.identity_file
        && authenticate_with_key_file(handle, &entry.username, identity_file).await?
    {
        return Ok(true);
    }

    Ok(false)
}

async fn authenticate_with_agent(
    handle: &mut Handle<TermConnectHandler>,
    username: &str,
) -> Result<bool> {
    let Ok(mut agent) = AgentClient::connect_env().await else {
        return Ok(false);
    };

    let Ok(identities) = agent.request_identities().await else {
        return Ok(false);
    };

    for identity in identities {
        let public_key = identity.public_key().into_owned();
        let mut signer = AgentSigner(&mut agent);

        let result = handle
            .authenticate_publickey_with(username, public_key, None, &mut signer)
            .await;

        if let Ok(AuthResult::Success) = result {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn authenticate_with_key_file(
    handle: &mut Handle<TermConnectHandler>,
    username: &str,
    identity_file: &Path,
) -> Result<bool> {
    let Ok(private_key) = PrivateKey::read_openssh_file(identity_file) else {
        return Ok(false);
    };

    if private_key.is_encrypted() {
        // Passphrase-protected keys aren't prompted for yet; skip to the
        // next method rather than hanging or failing hard.
        return Ok(false);
    }

    let key = PrivateKeyWithHashAlg::new(Arc::new(private_key), None);

    match handle.authenticate_publickey(username, key).await? {
        AuthResult::Success => Ok(true),
        AuthResult::Failure { .. } => Ok(false),
    }
}

/// Password authentication, used as the interactive fallback once
/// non-interactive methods are exhausted.
pub async fn authenticate_password(
    handle: &mut Handle<TermConnectHandler>,
    username: &str,
    password: &str,
) -> Result<bool> {
    match handle.authenticate_password(username, password).await? {
        AuthResult::Success => Ok(true),
        AuthResult::Failure { .. } => Ok(false),
    }
}

/// Adapts an agent connection to russh's [`Signer`] trait, so the agent can
/// sign an authentication challenge without the private key ever leaving
/// it.
struct AgentSigner<'a>(&'a mut AgentClient<tokio::net::UnixStream>);

impl Signer for AgentSigner<'_> {
    type Error = anyhow::Error;

    async fn auth_sign(
        &mut self,
        key: &AgentIdentity,
        hash_alg: Option<HashAlg>,
        to_sign: Vec<u8>,
    ) -> Result<Vec<u8>, Self::Error> {
        let public_key = key.public_key().into_owned();
        let signature = self
            .0
            .sign_request_signature(&public_key, hash_alg, &to_sign)
            .await?;
        Ok(signature.as_bytes().to_vec())
    }
}
