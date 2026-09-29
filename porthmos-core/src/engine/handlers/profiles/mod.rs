mod keyring;

use keyring::SecretChanges;
pub(crate) use keyring::{KeyringDone, KeyringJob, SecretOwner, profile_account, ssh_account};
use porthmos_vfs::{ConnectionForm, OptionKind};

use super::super::{Engine, Event};
use crate::{
    Severity,
    profiles::{self, ConnectionEntry, ConnectionSource, Labels, PASSWORD_MARKER, ProfileDraft, labels, store},
    secrets::SecretKey,
};

impl Engine {
    pub(crate) fn all_profiles(&self) -> anyhow::Result<Vec<ConnectionEntry>> {
        profiles::list_all(&self.paths, &self.protocols, &self.env)
    }

    pub(crate) fn list_profiles(&mut self) {
        match self.all_profiles() {
            Ok(entries) => self.emit(Event::Profiles(entries)),
            Err(err) => self.notice(Severity::Error, err.to_string()),
        }
    }

    pub(crate) fn save_profile(&mut self, original: Option<String>, draft: ProfileDraft) {
        let preserve_from =
            original.as_ref().and_then(|name| self.all_profiles().ok()?.into_iter().find(|entry| &entry.name == name));
        let registered = self.protocols.iter().find(|protocol| protocol.id() == draft.protocol);
        let unavailable = preserve_from.as_ref().filter(|entry| entry.protocol == draft.protocol);
        let form = match (registered, unavailable) {
            (Some(protocol), _) => protocol.connection_form(),
            (None, Some(entry)) => ConnectionForm::standard(entry.port),
            (None, None) => {
                self.emit(Event::ProfileRejected {
                    message: format!("No \"{}\" protocol is available", draft.protocol),
                });
                return;
            }
        };
        let profile = match draft.validate(&form, preserve_from.as_ref()) {
            Ok(profile) => profile,
            Err(message) => {
                self.emit(Event::ProfileRejected { message });
                return;
            }
        };

        match store::load(&self.paths) {
            Ok(existing)
                if existing
                    .iter()
                    .any(|saved| saved.name == profile.name && Some(&saved.name) != original.as_ref()) =>
            {
                self.emit(Event::ProfileRejected {
                    message: format!("A connection named \"{}\" already exists", profile.name),
                });
                return;
            }
            Ok(_) => {}
            Err(err) => {
                self.notice(Severity::Error, err.to_string());
                return;
            }
        }

        if let Some(original) = &original
            && profile.name != *original
            && let Err(err) = store::delete(&self.paths, original)
        {
            self.notice(Severity::Error, err.to_string());
            return;
        }

        match store::save(&self.paths, &profile) {
            Ok(()) => {
                self.emit(Event::ProfileSaved);
                self.list_profiles();
            }
            Err(err) => return self.notice(Severity::Error, err.to_string()),
        }

        let renamed_from = original.filter(|old| *old != profile.name);
        let saved_before = preserve_from.map(|entry| entry.in_keyring).unwrap_or_default();
        let secret_keys: Vec<&str> =
            form.options.iter().filter(|field| field.kind == OptionKind::Secret).map(|field| field.key).collect();
        let edits = std::iter::once((PASSWORD_MARKER.to_string(), draft.password))
            .chain(draft.secret_options.into_iter().filter(|(key, _)| secret_keys.contains(&key.as_str())));
        let changes =
            SecretChanges::from_edits(profile.name.clone(), renamed_from, saved_before, profile.in_keyring, edits);
        self.apply_secret_changes(changes);
    }

    fn is_ssh_host(&self, name: &str) -> anyhow::Result<bool> {
        Ok(self.all_profiles()?.iter().any(|entry| entry.name == name && entry.source == ConnectionSource::SshConfig))
    }

    pub(crate) fn save_ssh_labels(&mut self, name: &str, group: &str, tags: &str) {
        match self.is_ssh_host(name) {
            Ok(true) => {}
            Ok(false) => {
                self.emit(Event::ProfileRejected { message: format!("{name} is not in ~/.ssh/config") });
                return;
            }
            Err(err) => return self.notice(Severity::Error, err.to_string()),
        }
        let labels =
            Labels { group: labels::normalize_group(group), tags: labels::parse_tags(tags), in_keyring: Vec::new() };
        match store::save_ssh_labels(&self.paths, name, &labels) {
            Ok(()) => {
                self.emit(Event::ProfileSaved);
                self.list_profiles();
            }
            Err(err) => self.notice(Severity::Error, err.to_string()),
        }
    }

    pub(crate) fn move_ssh_labels(&mut self, from: &str, to: &str) {
        match self.is_ssh_host(to) {
            Ok(true) => {
                let saved = self.ssh_markers(from);
                match store::move_ssh_labels(&self.paths, from, to) {
                    Ok(()) if !saved.is_empty() => self.move_ssh_password(from, to, saved),
                    Ok(()) => {}
                    Err(err) => self.notice(Severity::Error, err.to_string()),
                }
            }
            Ok(false) => self.notice(Severity::Error, format!("{to} is not in ~/.ssh/config")),
            Err(err) => self.notice(Severity::Error, err.to_string()),
        }
        self.list_profiles();
    }

    pub(crate) fn forget_ssh_labels(&mut self, name: &str) {
        let saved = self.ssh_markers(name);
        if let Err(err) = store::forget_ssh_labels(&self.paths, name) {
            self.notice(Severity::Error, err.to_string());
            return self.list_profiles();
        }
        self.list_profiles();
        self.drop_ssh_password(name, saved, false);
    }

    pub(crate) fn forget_ssh_password(&mut self, alias: &str) {
        let saved = self.ssh_markers(alias);
        if saved.is_empty() {
            self.secrets.uncache(&SecretKey::SshHost { alias: alias.to_string() }.account());
            return self.list_profiles();
        }
        self.drop_ssh_password(alias, saved, true);
    }

    pub(crate) fn delete_profile(&mut self, name: &str) {
        let saved = store::load(&self.paths)
            .ok()
            .and_then(|profiles| profiles.into_iter().find(|profile| profile.name == name))
            .map(|profile| profile.in_keyring)
            .unwrap_or_default();
        if let Err(err) = store::delete(&self.paths, name) {
            self.notice(Severity::Error, err.to_string());
            return self.list_profiles();
        }
        self.list_profiles();
        self.forget_profile_secrets(name.to_string(), saved);
    }
}

#[cfg(test)]
mod tests;
