use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;

use crate::tui::input::{self, Action};
use crate::tui::panels::{self, ActivePanel, PanelState};
use crate::tui::widgets::dialog::{ConfirmDialog, Dialog, DialogOutcome, TextInputDialog};
use crate::tui::{Backend, layout};

const HINT_TEXT: &str = "\u{2191}\u{2193} Navigate  Enter Open  Tab Switch  Space Select  F2 Rename  F7 Mkdir  F8 Delete  Ctrl+R Refresh  F10 Quit";

/// The file operation a dialog is currently collecting input/confirmation for.
enum PendingAction {
    Mkdir,
    Rename,
    Delete,
}

pub struct App {
    should_quit: bool,
    active_panel: ActivePanel,
    local: PanelState,
    dialog: Option<Dialog>,
    pending_action: Option<PendingAction>,
    status: Option<String>,
}

impl App {
    pub fn new() -> Result<Self> {
        Self::at(std::env::current_dir()?)
    }

    fn at(path: PathBuf) -> Result<Self> {
        Ok(Self {
            should_quit: false,
            active_panel: ActivePanel::Local,
            local: PanelState::new(path)?,
            dialog: None,
            pending_action: None,
            status: None,
        })
    }

    pub async fn run(&mut self, terminal: &mut ratatui::Terminal<Backend>) -> Result<()> {
        let mut events = EventStream::new();

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;

            if let Some(event) = events.next().await
                && let Event::Key(key) = event?
                && key.kind == KeyEventKind::Press
            {
                if self.dialog.is_some() {
                    self.apply_dialog_key(key);
                } else {
                    self.apply_action(input::map_key(key));
                }
            }
        }

        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let (main_area, status_area) = layout::split_frame(frame.area());
        let (local_area, remote_area) = layout::split_panels(main_area);

        panels::render_panel(
            frame,
            local_area,
            "LOCAL",
            self.active_panel == ActivePanel::Local,
            &self.local,
        );
        panels::render_placeholder(
            frame,
            remote_area,
            "REMOTE",
            self.active_panel == ActivePanel::Remote,
        );

        self.render_status(frame, status_area);

        if let Some(dialog) = &self.dialog {
            dialog.render(frame, frame.area());
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let (text, style) = match &self.status {
            Some(message) => (message.as_str(), Style::default().fg(Color::Red)),
            None => (HINT_TEXT, Style::default()),
        };

        frame.render_widget(Paragraph::new(text).style(style), area);
    }

    fn apply_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::SwitchPanel => self.active_panel.toggle(),
            Action::Mkdir => self.open_mkdir_dialog(),
            Action::Rename => self.open_rename_dialog(),
            Action::Delete => self.open_delete_dialog(),
            Action::Up | Action::Down | Action::ToggleSelect | Action::Open | Action::Refresh => {
                self.apply_panel_action(action);
            }
            Action::Noop => {}
        }
    }

    /// Actions that operate on whichever panel is focused. Only the local
    /// panel has real state so far (the remote panel arrives in Phase 4).
    fn apply_panel_action(&mut self, action: Action) {
        if self.active_panel != ActivePanel::Local {
            return;
        }

        let result = match action {
            Action::Up => {
                self.local.move_cursor(-1);
                Ok(())
            }
            Action::Down => {
                self.local.move_cursor(1);
                Ok(())
            }
            Action::ToggleSelect => {
                self.local.toggle_selection();
                Ok(())
            }
            Action::Open => self.local.open_selected(),
            Action::Refresh => self.local.refresh(),
            _ => Ok(()),
        };

        self.set_status(result);
    }

    fn open_mkdir_dialog(&mut self) {
        if self.active_panel != ActivePanel::Local {
            return;
        }

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new(
            "New directory name",
            "",
        )));
        self.pending_action = Some(PendingAction::Mkdir);
    }

    fn open_rename_dialog(&mut self) {
        if self.active_panel != ActivePanel::Local {
            return;
        }

        let Some(current_name) = self.local.current_entry_name() else {
            return;
        };

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new(
            "Rename to",
            current_name,
        )));
        self.pending_action = Some(PendingAction::Rename);
    }

    fn open_delete_dialog(&mut self) {
        if self.active_panel != ActivePanel::Local {
            return;
        }

        let targets = self.local.targets();
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

    fn apply_dialog_key(&mut self, key: KeyEvent) {
        let Some(dialog) = self.dialog.as_mut() else {
            return;
        };

        match dialog.handle_key(key) {
            DialogOutcome::Pending => {}
            DialogOutcome::Cancelled => {
                self.dialog = None;
                self.pending_action = None;
            }
            DialogOutcome::Confirmed => {
                self.dialog = None;
                if let Some(PendingAction::Delete) = self.pending_action.take() {
                    let result = self.local.delete_targets();
                    self.set_status(result);
                }
            }
            DialogOutcome::Submitted(value) => {
                self.dialog = None;
                let result = match self.pending_action.take() {
                    Some(PendingAction::Mkdir) => self.local.create_directory(&value),
                    Some(PendingAction::Rename) => self.local.rename_current(&value),
                    Some(PendingAction::Delete) | None => Ok(()),
                };
                self.set_status(result);
            }
        }
    }

    fn set_status(&mut self, result: Result<()>) {
        self.status = result.err().map(|err| err.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
    use std::fs;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn app_in_temp_dir() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let app = App::at(dir.path().to_path_buf()).unwrap();
        (dir, app)
    }

    #[test]
    fn quit_action_sets_should_quit() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn switch_panel_action_toggles_active_panel() {
        let (_dir, mut app) = app_in_temp_dir();
        assert_eq!(app.active_panel, ActivePanel::Local);
        app.apply_action(Action::SwitchPanel);
        assert_eq!(app.active_panel, ActivePanel::Remote);
    }

    #[test]
    fn noop_action_does_not_change_state() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::Noop);
        assert!(!app.should_quit);
        assert_eq!(app.active_panel, ActivePanel::Local);
    }

    #[test]
    fn panel_actions_are_ignored_when_remote_panel_is_focused() {
        let (dir, mut app) = app_in_temp_dir();
        fs::create_dir(dir.path().join("child")).unwrap();
        app.apply_action(Action::SwitchPanel);

        let cursor_before = app.local.cursor;
        app.apply_action(Action::Down);

        assert_eq!(app.local.cursor, cursor_before);
    }

    #[test]
    fn mkdir_dialog_creates_a_directory_on_submit() {
        let (dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::Mkdir);
        assert!(app.dialog.is_some());

        for c in "new_dir".chars() {
            app.apply_dialog_key(key(KeyCode::Char(c)));
        }
        app.apply_dialog_key(key(KeyCode::Enter));

        assert!(app.dialog.is_none());
        assert!(dir.path().join("new_dir").is_dir());
    }

    #[test]
    fn mkdir_dialog_cancelled_with_esc_creates_nothing() {
        let (dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::Mkdir);
        app.apply_dialog_key(key(KeyCode::Char('x')));
        app.apply_dialog_key(key(KeyCode::Esc));

        assert!(app.dialog.is_none());
        assert!(!dir.path().join("x").exists());
    }

    #[test]
    fn delete_dialog_confirmed_with_y_deletes_the_target() {
        let (dir, mut app) = app_in_temp_dir();
        fs::write(dir.path().join("doomed.txt"), b"content").unwrap();
        app.local.refresh().unwrap();
        app.local.cursor = app.local.rows().len() - 1;

        app.apply_action(Action::Delete);
        assert!(app.dialog.is_some());
        app.apply_dialog_key(key(KeyCode::Char('y')));

        assert!(app.dialog.is_none());
        assert!(!dir.path().join("doomed.txt").exists());
    }

    #[test]
    fn delete_dialog_cancelled_with_n_deletes_nothing() {
        let (dir, mut app) = app_in_temp_dir();
        fs::write(dir.path().join("safe.txt"), b"content").unwrap();
        app.local.refresh().unwrap();
        app.local.cursor = app.local.rows().len() - 1;

        app.apply_action(Action::Delete);
        app.apply_dialog_key(key(KeyCode::Char('n')));

        assert!(app.dialog.is_none());
        assert!(dir.path().join("safe.txt").exists());
    }

    #[test]
    fn rename_dialog_prefills_the_current_name() {
        let (dir, mut app) = app_in_temp_dir();
        fs::write(dir.path().join("old.txt"), b"content").unwrap();
        app.local.refresh().unwrap();
        app.local.cursor = app.local.rows().len() - 1;

        app.apply_action(Action::Rename);

        match app.dialog {
            Some(Dialog::TextInput(ref dialog)) => assert_eq!(dialog.value, "old.txt"),
            _ => panic!("expected a text input dialog"),
        }
    }

    #[test]
    fn failed_operation_sets_a_status_message() {
        let (_dir, mut app) = app_in_temp_dir();
        // Renaming when nothing is selected/under the cursor is a no-op that
        // succeeds trivially, so instead force a real failure: try to create
        // a directory that already exists.
        app.local.create_directory("dup").unwrap();
        app.apply_action(Action::Mkdir);
        for c in "dup".chars() {
            app.apply_dialog_key(key(KeyCode::Char(c)));
        }
        app.apply_dialog_key(key(KeyCode::Enter));

        assert!(app.status.is_some());
    }
}
