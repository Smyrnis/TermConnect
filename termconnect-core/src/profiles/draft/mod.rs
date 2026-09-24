use std::fmt;

use super::{ConnectionEntry, ConnectionProfile, profile::DEFAULT_PROTOCOL};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProfileDraft {
    pub name: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for ProfileDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileDraft")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

const INVALID_PORT: &str = "Port must be a number from 1-65535";

impl ProfileDraft {
    pub fn validate(&self, preserve_from: Option<&ConnectionEntry>) -> Result<ConnectionProfile, String> {
        let name = self.name.trim();
        let host = self.host.trim();
        let username = self.username.trim();

        if name.is_empty() {
            return Err("Name can't be empty".to_string());
        }
        if host.is_empty() {
            return Err("Host can't be empty".to_string());
        }
        if username.is_empty() {
            return Err("Username can't be empty".to_string());
        }
        let port: u16 = self.port.trim().parse().map_err(|_| INVALID_PORT.to_string())?;
        if port == 0 {
            return Err(INVALID_PORT.to_string());
        }
        let password = if self.password.is_empty() { None } else { Some(self.password.clone()) };

        Ok(ConnectionProfile {
            name: name.to_string(),
            protocol: preserve_from.map(|entry| entry.protocol.clone()).unwrap_or_else(|| DEFAULT_PROTOCOL.to_string()),
            host: host.to_string(),
            port: Some(port),
            username: username.to_string(),
            password,
            options: preserve_from.map(|entry| entry.options.clone()).unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests;
