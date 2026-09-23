use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    #[serde(skip)]
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub identity_file: Option<PathBuf>,
    #[serde(default)]
    pub remote_path: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

impl std::fmt::Debug for ConnectionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionProfile")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("identity_file", &self.identity_file)
            .field("remote_path", &self.remote_path)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    Profile,
    SshConfig,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectionEntry {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub identity_file: Option<PathBuf>,
    pub remote_path: Option<String>,
    pub password: Option<String>,
    pub source: ConnectionSource,
}

impl std::fmt::Debug for ConnectionEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionEntry")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("identity_file", &self.identity_file)
            .field("remote_path", &self.remote_path)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("source", &self.source)
            .finish()
    }
}

impl From<ConnectionProfile> for ConnectionEntry {
    fn from(profile: ConnectionProfile) -> Self {
        Self {
            name: profile.name,
            host: profile.host,
            port: profile.port,
            username: profile.username,
            identity_file: profile.identity_file,
            remote_path: profile.remote_path,
            password: profile.password,
            source: ConnectionSource::Profile,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/connection/profile_test.rs"]
mod tests;
