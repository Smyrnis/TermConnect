use super::super::{Engine, Event, Internal};
use crate::{Severity, state};

impl Engine {
    pub(crate) fn start_keyring_probe(&self) {
        let (secrets, internal) = (self.secrets.clone(), self.internal.clone());
        tokio::spawn(async move {
            let available = secrets.probe().await;
            let _ = internal.send(Internal::KeyringProbed { available });
        });
    }

    pub(crate) fn publish_save_choice(&self) {
        self.emit(Event::SaveChoice { save: state::load(&self.paths).save_passwords_in_keyring });
    }

    pub(crate) fn remember_save_choice(&mut self, save: bool) {
        if let Err(err) = state::save(&self.paths, &state::UiState { save_passwords_in_keyring: save }) {
            self.notice(Severity::Warning, format!("Couldn't remember the keyring choice: {err}"));
        }
    }
}

#[cfg(test)]
mod tests;
