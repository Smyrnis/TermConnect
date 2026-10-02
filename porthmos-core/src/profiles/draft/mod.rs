use std::{collections::BTreeMap, fmt};

use porthmos_vfs::{ConnectionForm, OptionKind};

use super::{ConnectionEntry, ConnectionProfile, PASSWORD_MARKER, labels};

const REMOTE_PATH: &str = "remote_path";

#[derive(Clone, Default, PartialEq, Eq)]
pub enum SecretEdit {
    #[default]
    Keep,
    Replace(String),
    Clear,
}

impl fmt::Debug for SecretEdit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretEdit::Keep => write!(f, "Keep"),
            SecretEdit::Replace(_) => write!(f, "Replace(<redacted>)"),
            SecretEdit::Clear => write!(f, "Clear"),
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProfileDraft {
    pub protocol: String,
    pub name: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: SecretEdit,
    pub remote_path: String,
    pub group: String,
    pub tags: String,
    pub options: BTreeMap<String, String>,
    pub secret_options: BTreeMap<String, SecretEdit>,
}

impl fmt::Debug for ProfileDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileDraft")
            .field("protocol", &self.protocol)
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password)
            .field("remote_path", &self.remote_path)
            .field("group", &self.group)
            .field("tags", &self.tags)
            .field("options", &self.options.keys().collect::<Vec<_>>())
            .field("secret_options", &self.secret_options)
            .finish()
    }
}

fn required(value: &str, label: &str, is_required: bool) -> Result<(), String> {
    if is_required && value.is_empty() { Err(format!("{label} can't be empty")) } else { Ok(()) }
}

fn has_marker(original: Option<&ConnectionEntry>, marker: &str) -> bool {
    original.is_some_and(|entry| entry.in_keyring.iter().any(|saved| saved == marker))
}

fn required_secret(edit: &SecretEdit, label: &str, is_required: bool, saved: bool, trim: bool) -> Result<(), String> {
    match edit {
        SecretEdit::Replace(value) => required(if trim { value.trim() } else { value }, label, is_required),
        SecretEdit::Keep if saved => Ok(()),
        SecretEdit::Keep | SecretEdit::Clear => required("", label, is_required),
    }
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
        let original = preserve_from.filter(|entry| entry.protocol == self.protocol);
        let password_saved = has_marker(original, PASSWORD_MARKER);
        required_secret(&self.password, form.password.label, form.password.required, password_saved, false)?;
        let port = self.port(form)?;

        let mut in_keyring = Vec::new();
        if password_saved && self.password == SecretEdit::Keep {
            in_keyring.push(PASSWORD_MARKER.to_string());
        }
        let mut options = BTreeMap::new();
        if let Some(original) = original {
            for (key, value) in &original.options {
                if key != REMOTE_PATH && form.option(key).is_none() {
                    options.insert(key.clone(), value.clone());
                }
            }
        }
        for field in &form.options {
            if field.kind == OptionKind::Secret {
                let edit = self.secret_options.get(field.key).cloned().unwrap_or_default();
                let saved = has_marker(original, field.key);
                required_secret(&edit, field.label, field.required, saved, true)?;
                if saved && edit == SecretEdit::Keep {
                    in_keyring.push(field.key.to_string());
                }
                continue;
            }
            let raw = self.options.get(field.key).map(String::as_str).unwrap_or_default();
            required(raw.trim(), field.label, field.required)?;
            if raw.trim().is_empty() {
                continue;
            }
            let value = raw.trim();
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
            group: labels::normalize_group(&self.group),
            tags: labels::parse_tags(&self.tags),
            in_keyring,
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
