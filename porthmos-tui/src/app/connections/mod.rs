use porthmos_core::profiles::ProfileDraft;

use super::*;
use crate::widgets::dialog::{FormDialog, FormField};

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

        self.dialog = Some(build_connection_form("Add connection", None));
        self.pending_action = Some(PendingAction::AddConnection);
    }

    pub(super) fn submit_connection_form(&mut self, values: Vec<String>, original: Option<String>) {
        match draft_from_form(&values) {
            Ok(draft) => self.core.send(Command::SaveProfile { original, draft }),
            Err(message) => self.set_form_error(message),
        }
    }

    pub(super) fn open_edit_connection_dialog(&mut self) {
        if self.screen != Screen::Connections {
            return;
        }
        let Some(entry) = self.connections.get(self.connections_cursor).cloned() else {
            return;
        };
        if entry.source == ConnectionSource::SshConfig {
            self.notifications
                .push(Severity::Info, "This connection is defined in ~/.ssh/config and can't be edited here");
            return;
        }

        self.dialog = Some(build_connection_form("Edit connection", Some(&entry)));
        self.pending_action = Some(PendingAction::EditConnection { original: entry });
    }

    pub(super) fn open_delete_connection_dialog(&mut self) {
        if self.screen != Screen::Connections {
            return;
        }
        let Some(entry) = self.connections.get(self.connections_cursor) else {
            return;
        };
        if entry.source == ConnectionSource::SshConfig {
            self.notifications
                .push(Severity::Info, "This connection is defined in ~/.ssh/config and can't be deleted here");
            return;
        }

        let name = entry.name.clone();
        self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(format!("Delete connection \"{name}\"?"))));
        self.pending_action = Some(PendingAction::DeleteConnection { name });
    }

    pub(super) fn set_form_error(&mut self, message: String) {
        if let Some(Dialog::Form(form)) = self.dialog.as_mut() {
            form.error = Some(message);
        }
    }

    pub(super) fn connect_to_selected(&mut self) {
        let Some(entry) = self.connections.get(self.connections_cursor) else {
            return;
        };

        if let Some(session) = self.sessions.by_name(&entry.name) {
            self.sessions.activate(session.id);
            return;
        }

        self.core.send(Command::Connect { profile: entry.name.clone() });
    }

    pub(super) fn disconnect_selected(&mut self) {
        let Some(entry) = self.connections.get(self.connections_cursor) else {
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

fn draft_from_form(values: &[String]) -> Result<ProfileDraft, String> {
    let [name, host, port, username, password] = values else {
        return Err("Unexpected number of fields".to_string());
    };
    Ok(ProfileDraft {
        name: name.clone(),
        host: host.clone(),
        port: port.clone(),
        username: username.clone(),
        password: password.clone(),
    })
}

fn build_connection_form(title: &str, existing: Option<&ConnectionEntry>) -> Dialog {
    let (name, host, port, username, password) = match existing {
        Some(entry) => (
            entry.name.clone(),
            entry.host.clone(),
            entry.port.to_string(),
            entry.username.clone(),
            entry.password.clone().unwrap_or_default(),
        ),
        None => (String::new(), String::new(), "22".to_string(), String::new(), String::new()),
    };

    Dialog::Form(FormDialog::new(
        title,
        vec![
            FormField::new("Name", name),
            FormField::new("Host", host),
            FormField::new("Port", port),
            FormField::new("Username", username),
            FormField::new_masked("Password", password),
        ],
    ))
}

#[cfg(test)]
mod tests;
