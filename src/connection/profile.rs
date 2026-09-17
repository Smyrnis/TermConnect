use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A saved connection, as stored in `~/.config/termconnect/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

fn default_port() -> u16 {
    22
}

/// Where a [`ConnectionEntry`] came from — only `Profile`-sourced entries
/// can be edited or deleted from the app; `SshConfig` entries are read-only,
/// since `~/.ssh/config` isn't a file this app owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    Profile,
    SshConfig,
}

/// A connection ready to be dialed: either a saved profile or an entry
/// discovered in `~/.ssh/config`, unified into one shape so the UI doesn't
/// need to care where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
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
