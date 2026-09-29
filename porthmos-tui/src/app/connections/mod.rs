mod form;
mod missing;

use porthmos_core::Answer;

use super::*;
use crate::widgets::dialog::{FormDialog, FormField};

const NO_KEYRING_HINT: &str = "Passwords are not saved: no system keyring. They are kept until Porthmos quits.";

impl App {
    pub(super) fn request_shell(&mut self) {
        let Some(session) = self.sessions.active() else {
            self.notifications.push(Severity::Warning, "Connect to a server first");
            return;
        };
        if !session.shell_available {
            self.notifications.push(Severity::Warning, "This connection doesn't support a terminal session");
            return;
        }
        self.core.send(Command::PrepareShell { session: session.id });
    }

    pub(super) async fn launch_shell(
        &mut self, terminal: &mut ratatui::Terminal<Backend>, invocation: ShellInvocation,
    ) -> Result<()> {
        terminal::restore()?;

        let ssh_result = tokio::task::spawn_blocking(move || invocation.to_command().status()).await;

        *terminal = terminal::init()?;

        match ssh_result {
            Ok(Ok(status)) if !status.success() => {
                self.notifications.push(Severity::Error, format!("ssh exited with status {status}"));
            }
            Ok(Ok(_)) => {}
            Ok(Err(io_err)) => {
                self.notifications.push(Severity::Error, format!("Failed to launch ssh: {io_err}"));
            }
            Err(join_err) => {
                self.notifications.push(Severity::Error, format!("ssh task failed: {join_err}"));
            }
        }

        Ok(())
    }

    pub(super) fn open_connections_screen(&mut self) {
        self.screen = Screen::Connections;
        self.core.send(Command::ListProfiles);
    }

    pub(super) fn open_add_connection_dialog(&mut self) {
        if self.screen != Screen::Connections {
            return;
        }

        let Some(mut dialog) = form::build("Add connection", self.core.protocols(), None) else {
            self.notifications.push(Severity::Warning, "No protocols are available in this build");
            return;
        };
        dialog.hint = self.keyring_hint();
        self.dialog = Some(Dialog::Form(dialog));
        self.pending_action = Some(PendingAction::AddConnection);
    }

    pub(super) fn form_choice_changed(&mut self, key: &'static str) {
        if key != "protocol" {
            return;
        }
        let core = self.core.clone();
        if let Some(Dialog::Form(dialog)) = self.dialog.as_mut() {
            form::rebuild_for_protocol(dialog, core.protocols());
        }
    }

    pub(super) fn submit_connection_form(&mut self, values: Vec<(&'static str, String)>, original: Option<String>) {
        self.reveal = values
            .iter()
            .find(|(key, _)| *key == "name")
            .map(|(_, name)| Reveal::AwaitingSave(name.trim().to_string()));
        let protocol = values.iter().find(|(key, _)| *key == "protocol").map(|(_, value)| value.as_str()).unwrap_or("");
        let secret_keys = form::secret_keys(self.core.protocols(), protocol);
        self.core.send(Command::SaveProfile { original, draft: Box::new(form::draft(values, &secret_keys)) });
    }

    pub(super) fn open_edit_connection_dialog(&mut self) {
        if self.screen != Screen::Connections {
            return;
        }
        let Some(entry) = self.connections.selected_entry().cloned() else {
            return;
        };
        if entry.source.is_orphan_labels() {
            return;
        }
        if entry.source == ConnectionSource::SshConfig {
            let name = entry.name.clone();
            self.dialog = Some(Dialog::Form(form::build_labels(&entry)));
            self.pending_action = Some(PendingAction::EditSshLabels { name });
            return;
        }

        let Some(mut dialog) = form::build("Edit connection", self.core.protocols(), Some(&entry)) else {
            return;
        };
        dialog.hint = self.keyring_hint();
        self.dialog = Some(Dialog::Form(dialog));
        self.pending_action = Some(PendingAction::EditConnection { original: entry });
    }

    pub(super) fn open_delete_connection_dialog(&mut self) {
        if self.screen != Screen::Connections {
            return;
        }
        let Some(entry) = self.connections.selected_entry() else {
            return;
        };
        if entry.source.is_orphan_labels() {
            return;
        }
        if entry.source == ConnectionSource::SshConfig {
            self.notifications
                .push(Severity::Info, "This connection is defined in ~/.ssh/config and can't be deleted here");
            return;
        }

        let name = entry.name.clone();
        self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(format!("Delete connection \"{name}\"?"))));
        self.pending_action = Some(PendingAction::DeleteConnection { name });
    }

    pub(super) fn submit_ssh_labels(&mut self, name: String, values: Vec<(&'static str, String)>) {
        let value = |wanted: &str| {
            values.iter().find(|(key, _)| *key == wanted).map(|(_, value)| value.clone()).unwrap_or_default()
        };
        let (group, tags, forget) = (value("group"), value("tags"), value("saved_password") == "forget");
        self.reveal = Some(Reveal::AwaitingSave(name.clone()));
        self.core.send(Command::SaveSshLabels { name: name.clone(), group, tags });
        if forget {
            self.core.send(Command::ForgetSshPassword { alias: name });
        }
    }

    fn keyring_hint(&self) -> Option<String> {
        (!self.keyring_available).then(|| NO_KEYRING_HINT.to_string())
    }

    pub(super) fn open_password_prompt(&mut self, request_id: RequestId, username: String, name: String) {
        let mut fields = vec![FormField::masked("password", "Password", "")];
        if self.keyring_available {
            let choices = vec![("true".to_string(), "Yes".to_string()), ("false".to_string(), "No".to_string())];
            let current = if self.save_choice { "true" } else { "false" };
            fields.push(FormField::choice("save", "Save in keyring", choices, current));
        }
        self.dialog = Some(Dialog::Form(FormDialog::new(format!("Password for {username}@{name}"), fields)));
        self.pending_action = Some(PendingAction::SubmitPassword { request_id });
    }

    pub(super) fn submit_password(&mut self, request_id: RequestId, values: Vec<(&'static str, String)>) {
        let value = |wanted: &str| values.iter().find(|(key, _)| *key == wanted).map(|(_, value)| value.clone());
        let save = value("save").as_deref() == Some("true");
        if value("save").is_some() && save != self.save_choice {
            self.save_choice = save;
            self.core.send(Command::RememberSaveChoice { save });
        }
        let password = value("password").unwrap_or_default();
        self.core.send(Command::Answer { request_id, answer: Some(Answer::Password(password)), save });
        self.dialog = None;
        self.pending_action = None;
    }

    pub(super) fn set_form_error(&mut self, message: String) {
        if let Some(Dialog::Form(form)) = self.dialog.as_mut() {
            form.error = Some(message);
        }
    }

    pub(super) fn connect_to_selected(&mut self) {
        let Some(entry) = self.connections.selected_entry() else {
            return;
        };

        if let Some(session) = self.sessions.by_name(&entry.name) {
            self.sessions.activate(session.id);
            return;
        }

        self.core.send(Command::Connect { profile: entry.name.clone() });
    }

    pub(super) fn disconnect_selected(&mut self) {
        let Some(entry) = self.connections.selected_entry().filter(|entry| !entry.source.is_orphan_labels()) else {
            return;
        };
        let Some(session) = self.sessions.by_name(&entry.name).map(|session| session.id) else {
            return;
        };

        self.core.send(Command::Disconnect { session });
    }

    pub(super) fn apply_connected(&mut self, session: u64, name: String, shell_available: bool) {
        self.connection_status = ConnectionStatus::Disconnected;
        if self.sessions.activate(session) {
            return;
        }
        let placeholder_panel = PanelView::from_listing(PathBuf::from("/"), Vec::new());
        self.sessions.insert(session, name, shell_available, placeholder_panel);
    }
}

#[cfg(test)]
mod tests;
