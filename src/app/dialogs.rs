use super::*;

impl App {
    pub(super) fn open_mkdir_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }
        if self.active_panel == ActivePanel::Remote && self.sessions.active().is_none() {
            return;
        }

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new(
            "New directory name",
            "",
        )));
        self.pending_action = Some(PendingAction::Mkdir);
    }

    pub(super) fn open_rename_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let current_name = match self.active_panel {
            ActivePanel::Local => self.local.current_entry_name(),
            ActivePanel::Remote => self
                .sessions
                .active()
                .and_then(|session| session.panel.current_entry_name()),
        };
        let Some(current_name) = current_name else {
            return;
        };

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new(
            "Rename to",
            current_name,
        )));
        self.pending_action = Some(PendingAction::Rename);
    }

    pub(super) fn open_delete_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let targets = match self.active_panel {
            ActivePanel::Local => self.local.targets(),
            ActivePanel::Remote => match self.sessions.active() {
                Some(session) => session.panel.targets(),
                None => return,
            },
        };
        if targets.is_empty() {
            return;
        }

        let message = if targets.len() == 1 {
            let name = targets[0]
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?");
            format!("Delete \"{name}\"?")
        } else {
            format!("Delete {} selected items?", targets.len())
        };

        self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(message)));
        self.pending_action = Some(PendingAction::Delete);
    }

    pub(super) fn apply_dialog_key(&mut self, key: KeyEvent) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };

        match dialog.handle_key(key) {
            DialogOutcome::Pending => {}
            DialogOutcome::Cancelled => {
                self.dialog = None;
                if let Some(PendingAction::SubmitPassword) = self.pending_action.take() {
                    // Dropping the sender signals cancellation to the
                    // waiting connect task.
                    self.pending_password = None;
                    self.connection_status = ConnectionStatus::Disconnected;
                }
            }
            DialogOutcome::Confirmed => {
                self.dialog = None;
                if let Some(PendingAction::Delete) = self.pending_action.take() {
                    match self.active_panel {
                        ActivePanel::Local => {
                            let result = self.local.delete_targets();
                            self.set_status(result);
                        }
                        ActivePanel::Remote => self.spawn_remote_delete(),
                    }
                }
            }
            DialogOutcome::Submitted(value) => {
                self.dialog = None;
                match self.pending_action.take() {
                    Some(PendingAction::Mkdir) => match self.active_panel {
                        ActivePanel::Local => {
                            let result = self.local.create_directory(&value);
                            self.set_status(result);
                        }
                        ActivePanel::Remote => self.spawn_remote_mkdir(value),
                    },
                    Some(PendingAction::Rename) => match self.active_panel {
                        ActivePanel::Local => {
                            let result = self.local.rename_current(&value);
                            self.set_status(result);
                        }
                        ActivePanel::Remote => self.spawn_remote_rename(value),
                    },
                    Some(PendingAction::SubmitPassword) => {
                        if let Some(sender) = self.pending_password.take() {
                            let _ = sender.send(value);
                        }
                    }
                    Some(PendingAction::AddBookmark) => self.add_bookmark(value),
                    Some(PendingAction::Delete) | None => {}
                }
            }
            DialogOutcome::Selected(index) => {
                self.dialog = None;
                self.navigate_to_bookmark(index);
            }
            DialogOutcome::Removed(index) => {
                if let Some(removed) = self.bookmarks.remove(index) {
                    self.save_bookmarks();
                    self.notifications.push(
                        Severity::Info,
                        format!("Removed bookmark \"{}\"", removed.label),
                    );
                }
                if let Some(Dialog::List(list)) = self.dialog.as_mut() {
                    list.items.remove(index);
                    if list.cursor >= list.items.len() {
                        list.cursor = list.items.len().saturating_sub(1);
                    }
                }
            }
            DialogOutcome::FormSubmitted(_) => {
                self.dialog = None;
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/dialogs_test.rs"]
mod tests;
