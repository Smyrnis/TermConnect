use porthmos_core::Answer;

use super::*;

impl App {
    pub(super) fn open_mkdir_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }
        if self.active_panel == ActivePanel::Remote && self.sessions.active().is_none() {
            return;
        }

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new("New directory name", "")));
        self.pending_action = Some(PendingAction::Mkdir);
    }

    pub(super) fn open_rename_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let current_name = self.active_location().and_then(|location| self.panel(location)?.current_entry_name());
        let Some(current_name) = current_name else {
            return;
        };

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new("Rename to", current_name)));
        self.pending_action = Some(PendingAction::Rename);
    }

    pub(super) fn open_delete_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let Some(targets) = self.active_location().and_then(|location| Some(self.panel(location)?.targets())) else {
            return;
        };
        if targets.is_empty() {
            return;
        }

        let message = if targets.len() == 1 {
            let name = targets[0].file_name().and_then(|name| name.to_str()).unwrap_or("?");
            format!("Delete \"{name}\"?")
        } else {
            format!("Delete {} selected items?", targets.len())
        };

        self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(message)));
        self.pending_action = Some(PendingAction::Delete);
    }

    fn create_directory(&mut self, name: &str) {
        let Some(location) = self.active_location() else {
            return;
        };
        let Some(panel) = self.panel(location) else {
            return;
        };
        let path = panel.path().join(name);
        self.core.send(Command::CreateDir { location, path });
    }

    fn rename_current(&mut self, new_name: &str) {
        let Some(location) = self.active_location() else {
            return;
        };
        let Some(panel) = self.panel(location) else {
            return;
        };
        let Some(current_name) = panel.current_entry_name() else {
            return;
        };
        let from = panel.path().join(current_name);
        let to = panel.path().join(new_name);
        self.core.send(Command::Rename { location, from, to });
    }

    fn delete_targets(&mut self) {
        let Some(location) = self.active_location() else {
            return;
        };
        let Some(panel) = self.panel(location) else {
            return;
        };
        let paths = panel.targets();
        self.core.send(Command::Delete { location, paths });
    }

    pub(super) fn apply_dialog_key(&mut self, key: KeyEvent) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };

        match dialog.handle_key(key) {
            DialogOutcome::Pending => {}
            DialogOutcome::Cancelled => {
                self.dialog = None;
                if let Some(PendingAction::SubmitPassword { request_id } | PendingAction::TrustHostKey { request_id }) =
                    self.pending_action.take()
                {
                    self.core.send(Command::Answer { request_id, answer: None });
                    self.connection_status = ConnectionStatus::Disconnected;
                }
            }
            DialogOutcome::Confirmed => {
                self.dialog = None;
                match self.pending_action.take() {
                    Some(PendingAction::Delete) => self.delete_targets(),
                    Some(PendingAction::DeleteConnection { name }) => {
                        self.core.send(Command::DeleteProfile { name });
                    }
                    Some(PendingAction::TrustHostKey { request_id }) => {
                        self.core.send(Command::Answer { request_id, answer: Some(Answer::Confirmed) });
                    }
                    _ => {}
                }
            }
            DialogOutcome::Submitted(value) => {
                self.dialog = None;
                match self.pending_action.take() {
                    Some(PendingAction::Mkdir) => self.create_directory(&value),
                    Some(PendingAction::Rename) => self.rename_current(&value),
                    Some(PendingAction::SubmitPassword { request_id }) => {
                        self.core.send(Command::Answer { request_id, answer: Some(Answer::Password(value)) });
                    }
                    Some(PendingAction::AddBookmark) => self.add_bookmark(value),
                    Some(PendingAction::Delete)
                    | Some(PendingAction::TrustHostKey { .. })
                    | Some(PendingAction::AddConnection)
                    | Some(PendingAction::EditConnection { .. })
                    | Some(PendingAction::DeleteConnection { .. })
                    | Some(PendingAction::ResolveConflict)
                    | None => {}
                }
            }
            DialogOutcome::Selected(index) => {
                self.dialog = None;
                self.navigate_to_bookmark(index);
            }
            DialogOutcome::Removed(index) => {
                self.core.send(Command::RemoveBookmark { index });
                if let Some(Dialog::List(list)) = self.dialog.as_mut() {
                    list.items.remove(index);
                    if list.cursor >= list.items.len() {
                        list.cursor = list.items.len().saturating_sub(1);
                    }
                }
            }
            DialogOutcome::FormSubmitted(values) => match &self.pending_action {
                Some(PendingAction::AddConnection) => self.submit_connection_form(values, None),
                Some(PendingAction::EditConnection { original }) => {
                    let original = original.name.clone();
                    self.submit_connection_form(values, Some(original));
                }
                _ => self.dialog = None,
            },
            DialogOutcome::Resolved { resolution, apply_to_rest } => {
                self.dialog = None;
                self.pending_action = None;
                self.answer_conflict(resolution, apply_to_rest);
            }
        }
        self.open_next_conflict_prompt();
    }
}

#[cfg(test)]
mod tests;
