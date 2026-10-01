use super::super::{Engine, Event, Internal};
use crate::{Severity, state, tasks::Scope};

impl Engine {
    pub(crate) fn start_keyring_probe(&self) {
        let (secrets, internal) = (self.secrets.clone(), self.internal.clone());
        self.tasks.spawn("keyring-probe", Scope::Background, move |_| async move {
            let available = secrets.probe().await;
            let _ = internal.send(Internal::KeyringProbed { available });
        });
    }

    pub(crate) fn publish_save_choice(&mut self) {
        let loaded = state::load(&self.paths);
        if let Some(warning) = loaded.warning {
            self.report(Severity::Warning, warning);
        }
        self.state_protected = loaded.protected;
        self.emit(Event::SaveChoice { save: loaded.state.save_passwords_in_keyring });
    }

    pub(crate) fn remember_save_choice(&mut self, save: bool) {
        if self.state_protected {
            self.report(
                Severity::Warning,
                "Couldn't remember the keyring choice: the saved settings file is protected",
            );
            return;
        }
        if let Err(err) = state::save(&self.paths, &state::UiState { save_passwords_in_keyring: save }) {
            self.report(Severity::Warning, format!("Couldn't remember the keyring choice: {err}"));
        }
    }
}

#[cfg(test)]
mod tests;
