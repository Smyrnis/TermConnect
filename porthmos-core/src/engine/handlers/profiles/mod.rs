use porthmos_vfs::ConnectionForm;

use super::super::{Engine, Event};
use crate::{
    Severity,
    profiles::{self, ConnectionEntry, ProfileDraft, store},
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
            Err(err) => self.notice(Severity::Error, err.to_string()),
        }
    }

    pub(crate) fn delete_profile(&mut self, name: &str) {
        if let Err(err) = store::delete(&self.paths, name) {
            self.notice(Severity::Error, err.to_string());
        }
        self.list_profiles();
    }
}

#[cfg(test)]
mod tests;
