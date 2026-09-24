use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use termconnect_vfs::Target;

pub const DEFAULT_PROTOCOL: &str = "sftp";

fn default_protocol() -> String {
    DEFAULT_PROTOCOL.to_string()
}

fn is_default_protocol(protocol: &str) -> bool {
    protocol == DEFAULT_PROTOCOL
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    #[serde(skip)]
    pub name: String,
    #[serde(default = "default_protocol", skip_serializing_if = "is_default_protocol")]
    pub protocol: String,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(flatten)]
    pub options: BTreeMap<String, String>,
}

impl std::fmt::Debug for ConnectionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionProfile")
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("options", &self.options)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    Profile,
    SshConfig,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectionEntry {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub options: BTreeMap<String, String>,
    pub source: ConnectionSource,
}

impl std::fmt::Debug for ConnectionEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionEntry")
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("options", &self.options)
            .field("source", &self.source)
            .finish()
    }
}

impl ConnectionEntry {
    pub fn from_profile(profile: ConnectionProfile, default_port: u16) -> Self {
        Self {
            name: profile.name,
            protocol: profile.protocol,
            host: profile.host,
            port: profile.port.unwrap_or(default_port),
            username: profile.username,
            password: profile.password,
            options: profile.options,
            source: ConnectionSource::Profile,
        }
    }

    pub fn discovered(protocol: &str, target: Target) -> Self {
        Self {
            name: target.name,
            protocol: protocol.to_string(),
            host: target.host,
            port: target.port,
            username: target.username,
            password: target.password,
            options: target.options,
            source: ConnectionSource::SshConfig,
        }
    }

    pub fn option(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(String::as_str)
    }

    pub fn target(&self) -> Target {
        Target {
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: self.password.clone(),
            options: self.options.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
