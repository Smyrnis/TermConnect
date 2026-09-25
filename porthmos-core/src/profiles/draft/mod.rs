use std::{collections::BTreeMap, fmt};

use porthmos_vfs::{ConnectionForm, OptionKind};

use super::{ConnectionEntry, ConnectionProfile};

const REMOTE_PATH: &str = "remote_path";

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProfileDraft {
    pub protocol: String,
    pub name: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub remote_path: String,
    pub options: BTreeMap<String, String>,
}

impl fmt::Debug for ProfileDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileDraft")
            .field("protocol", &self.protocol)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("remote_path", &self.remote_path)
            .field("options", &self.options.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn required(value: &str, label: &str, is_required: bool) -> Result<(), String> {
    if is_required && value.is_empty() { Err(format!("{label} can't be empty")) } else { Ok(()) }
}

impl ProfileDraft {
    pub fn validate(
        &self, form: &ConnectionForm, preserve_from: Option<&ConnectionEntry>,
    ) -> Result<ConnectionProfile, String> {
        let name = self.name.trim();
        let host = self.host.trim();
        let username = self.username.trim();

        required(name, "Name", true)?;
        required(host, form.host.label, form.host.required)?;
        required(username, form.username.label, form.username.required)?;
        required(&self.password, form.password.label, form.password.required)?;
        let port = self.port(form)?;

        let mut options = BTreeMap::new();
        if let Some(original) = preserve_from.filter(|entry| entry.protocol == self.protocol) {
            for (key, value) in &original.options {
                if key != REMOTE_PATH && form.option(key).is_none() {
                    options.insert(key.clone(), value.clone());
                }
            }
        }
        for field in &form.options {
            let raw = self.options.get(field.key).map(String::as_str).unwrap_or_default();
            required(raw.trim(), field.label, field.required)?;
            if raw.trim().is_empty() {
                continue;
            }
            let value = if field.kind == OptionKind::Secret { raw } else { raw.trim() };
            match &field.kind {
                OptionKind::Choice { choices, .. } if !choices.iter().any(|choice| choice.value == value) => {
                    let labels: Vec<&str> = choices.iter().map(|choice| choice.label).collect();
                    return Err(format!("{} must be one of: {}", field.label, labels.join(", ")));
                }
                OptionKind::Toggle { .. } if value != "true" && value != "false" => {
                    return Err(format!("{} must be on or off", field.label));
                }
                _ => {}
            }
            options.insert(field.key.to_string(), value.to_string());
        }
        let remote_path = self.remote_path.trim();
        if !remote_path.is_empty() {
            options.insert(REMOTE_PATH.to_string(), remote_path.to_string());
        }

        Ok(ConnectionProfile {
            name: name.to_string(),
            protocol: self.protocol.clone(),
            host: host.to_string(),
            port: Some(port),
            username: username.to_string(),
            password: if self.password.is_empty() { None } else { Some(self.password.clone()) },
            options,
        })
    }

    fn port(&self, form: &ConnectionForm) -> Result<u16, String> {
        let port = self.port.trim();
        if port.is_empty() {
            return Ok(form.port.default);
        }
        match port.parse::<u16>() {
            Ok(port) if port != 0 => Ok(port),
            _ => Err(format!("{} must be a number from 1-65535", form.port.label)),
        }
    }
}

#[cfg(test)]
mod tests;
