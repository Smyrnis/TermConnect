use std::{path::Path, process::ExitStatus};

use porthmos_core::edit::{EditorCommand, EditorExit};

use super::*;
use crate::widgets::history_view::printable;

#[derive(Clone)]
pub(super) struct EditPrompt {
    pub(super) edit_id: u64,
    pub(super) name: String,
    pub(super) kind: EditQuestionKind,
}

pub(super) fn editor_process(editor: &EditorCommand, file: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(&editor.program);
    command.args(&editor.args).arg(file);
    command
}

pub(super) fn editor_exit(result: std::io::Result<ExitStatus>) -> EditorExit {
    match result {
        Ok(status) if status.success() => EditorExit::Success,
        Ok(status) => EditorExit::Status(status.code().unwrap_or(-1)),
        Err(err) => EditorExit::LaunchFailed(err.to_string()),
    }
}

impl App {
    pub(super) fn start_edit(&mut self) {
        if self.screen != Screen::Files {
            return;
        }
        let Some(location) = self.active_location() else {
            self.notifications.push(Severity::Warning, "Connect to a remote server first");
            return;
        };
        let Some(panel) = self.panel(location) else {
            return;
        };
        match panel.current_entry() {
            Some(entry) if entry.is_dir => self.notifications.push(Severity::Warning, "Can't edit a folder"),
            Some(entry) => {
                let path = entry.path.clone();
                self.core.send(Command::EditFile { location, path });
            }
            None if panel.on_parent_row() => self.notifications.push(Severity::Warning, "Can't edit a folder"),
            None => {}
        }
    }

    pub(super) fn ask_edit_question(&mut self, edit_id: u64, name: &str, kind: EditQuestionKind) {
        self.edit_questions.push_back(EditPrompt { edit_id, name: printable(name), kind });
        self.open_next_edit_question();
    }

    pub(super) fn open_next_edit_question(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let Some(prompt) = self.edit_questions.pop_front() else {
            return;
        };
        let EditPrompt { edit_id, name, kind } = prompt.clone();
        match kind {
            EditQuestionKind::Upload => {
                let message = format!("Upload your changes to {name}?");
                self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(message)));
                self.pending_action = Some(PendingAction::EditUpload { edit_id });
            }
            EditQuestionKind::Conflict => {
                let title = format!("{name} changed on the server while you were editing");
                let items = vec!["Overwrite".to_string(), "Keep mine as a copy".to_string(), "Cancel".to_string()];
                let mut dialog = ListDialog::new(title, items);
                dialog.cursor = 1;
                self.dialog = Some(Dialog::List(dialog));
                self.pending_action = Some(PendingAction::EditConflict { edit_id });
            }
        }
        self.open_edit_question = Some(prompt);
    }

    pub(super) fn requeue_open_edit_question(&mut self) {
        if let Some(prompt) = self.open_edit_question.take()
            && self.dialog.is_some()
            && matches!(
                self.pending_action,
                Some(PendingAction::EditUpload { .. } | PendingAction::EditConflict { .. })
            )
        {
            self.edit_questions.push_front(prompt);
        }
    }

    pub(super) fn after_dialog_key(&mut self) {
        if !matches!(self.pending_action, Some(PendingAction::EditUpload { .. } | PendingAction::EditConflict { .. })) {
            self.open_edit_question = None;
        }
        self.open_next_edit_question();
    }

    pub(super) fn request_quit(&mut self) {
        if !self.edits_busy {
            self.should_quit = true;
            return;
        }
        let message = "An edit is still being saved to the server. Quit anyway? The remote file may be left \
                       incomplete; your copy stays in the state folder.";
        self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(message)));
        self.pending_action = Some(PendingAction::QuitWhileSaving);
    }

    pub(super) fn answer_edit_conflict(&mut self, edit_id: u64, index: usize) {
        let choice = match index {
            0 => EditChoice::Upload,
            1 => EditChoice::KeepCopy,
            _ => EditChoice::Cancel,
        };
        self.core.send(Command::ResolveEdit { edit_id, choice });
    }

    pub(super) async fn launch_editor(
        &mut self, terminal: &mut ratatui::Terminal<Backend>, edit_id: u64, file: PathBuf, editor: EditorCommand,
    ) -> Result<()> {
        if let Err(err) = terminal::restore() {
            self.core.send(Command::FinishEdit { edit_id, exit: EditorExit::LaunchFailed(err.to_string()) });
            return Err(err);
        }
        let result = tokio::task::spawn_blocking(move || editor_process(&editor, &file).status()).await;
        let restored = terminal::init();
        let exit = match result {
            Ok(result) => editor_exit(result),
            Err(err) => EditorExit::LaunchFailed(err.to_string()),
        };
        self.core.send(Command::FinishEdit { edit_id, exit });
        *terminal = restored?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
