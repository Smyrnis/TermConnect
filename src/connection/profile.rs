use std::path::PathBuf;

use serde::Deserialize;

/// A saved connection, as stored in `~/.config/termconnect/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
}

fn default_port() -> u16 {
    22
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
}

impl From<ConnectionProfile> for ConnectionEntry {
    fn from(profile: ConnectionProfile) -> Self {
        Self {
            name: profile.name,
            host: profile.host,
            port: profile.port,
            username: profile.username,
            identity_file: profile.identity_file,
        }
    }
}
