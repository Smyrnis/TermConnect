use std::collections::BTreeMap;

use porthmos_vfs::Target;
use serde::{Deserialize, Serialize};

use super::{PASSWORD_MARKER, labels::Labels};

pub const DEFAULT_PROTOCOL: &str = "sftp";

fn default_protocol() -> String {
    DEFAULT_PROTOCOL.to_string()
}

fn is_default_protocol(protocol: &str) -> bool {
    protocol == DEFAULT_PROTOCOL
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredProfile")]
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
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_keyring: Vec<String>,
    #[serde(flatten)]
    pub options: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct StoredProfile {
    #[serde(default = "default_protocol")]
    protocol: String,
    host: String,
    #[serde(default)]
    port: Option<u16>,
    username: String,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    in_keyring: Vec<String>,
    #[serde(flatten)]
    options: BTreeMap<String, String>,
}

impl From<StoredProfile> for ConnectionProfile {
    fn from(mut stored: StoredProfile) -> Self {
        stored.options.remove(PASSWORD_MARKER);
        Self {
            name: String::new(),
            protocol: stored.protocol,
            host: stored.host,
            port: stored.port,
            username: stored.username,
            group: stored.group,
            tags: stored.tags,
            in_keyring: stored.in_keyring,
            options: stored.options,
        }
    }
}

impl std::fmt::Debug for ConnectionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionProfile")
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("group", &self.group)
            .field("tags", &self.tags)
            .field("in_keyring", &self.in_keyring)
            .field("options", &self.options.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionSource {
    Profile,
    SshConfig,
    MissingSshHost,
    ShadowedSshHost,
}

impl ConnectionSource {
    pub fn is_orphan_labels(self) -> bool {
        matches!(self, ConnectionSource::MissingSshHost | ConnectionSource::ShadowedSshHost)
    }
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
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub saved_password: bool,
    pub in_keyring: Vec<String>,
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
            .field("options", &self.options.keys().collect::<Vec<_>>())
            .field("group", &self.group)
            .field("tags", &self.tags)
            .field("saved_password", &self.saved_password)
            .field("in_keyring", &self.in_keyring)
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
            password: None,
            options: profile.options,
            group: profile.group,
            tags: profile.tags,
            saved_password: profile.in_keyring.iter().any(|marker| marker == PASSWORD_MARKER),
            in_keyring: profile.in_keyring,
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
            group: None,
            tags: Vec::new(),
            saved_password: false,
            in_keyring: Vec::new(),
            source: ConnectionSource::SshConfig,
        }
    }

    pub fn orphan_labels(name: String, labels: Labels, source: ConnectionSource) -> Self {
        Self {
            name,
            protocol: DEFAULT_PROTOCOL.to_string(),
            host: String::new(),
            port: 0,
            username: String::new(),
            password: None,
            options: BTreeMap::new(),
            group: labels.group,
            tags: labels.tags,
            saved_password: false,
            in_keyring: Vec::new(),
            source,
        }
    }

    pub fn option(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(String::as_str)
    }

    pub fn start_path(&self) -> Option<&str> {
        self.option("remote_path").map(str::trim).filter(|path| !path.is_empty())
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
