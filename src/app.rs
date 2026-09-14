use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use tokio::sync::{mpsc, oneshot};

use crate::connection::client::TermConnectHandler;
use crate::connection::{self, ConnectionEntry};
use crate::tui::input::{self, Action};
use crate::tui::panels::{self, ActivePanel, PanelState};
use crate::tui::widgets::connections_list;
use crate::tui::widgets::dialog::{ConfirmDialog, Dialog, DialogOutcome, TextInputDialog};
use crate::tui::{Backend, layout};

const HINT_TEXT: &str = "\u{2191}\u{2193} Navigate  Enter Open  Tab Switch  Space Select  F2 Rename  F7 Mkdir  F8 Delete  Ctrl+R Refresh  F9 Connections  F10 Quit";

/// The file operation a dialog is currently collecting input/confirmation for.
enum PendingAction {
    Mkdir,
    Rename,
    Delete,
    SubmitPassword,
}

/// Which top-level screen is currently shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Files,
    Connections,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectionStatus {
    Disconnected,
    Connecting(String),
    Connected(String),
    Failed(String),
}

/// Progress reported by a background connection attempt (see
/// [`run_connect`]), delivered back to the event loop over a channel so the
/// TUI never blocks on network I/O.
enum ConnectEvent {
    Connected {
        name: String,
        handle: russh::client::Handle<TermConnectHandler>,
    },
    NeedsPassword {
        name: String,
        username: String,
        respond_to: oneshot::Sender<String>,
    },
    Failed {
        message: String,
    },
}

pub struct App {
    should_quit: bool,
    screen: Screen,
    active_panel: ActivePanel,
    local: PanelState,
    dialog: Option<Dialog>,
    pending_action: Option<PendingAction>,
    pending_password: Option<oneshot::Sender<String>>,
    status: Option<String>,
    connections: Vec<ConnectionEntry>,
    connections_cursor: usize,
    connection_status: ConnectionStatus,
    connection_handle: Option<russh::client::Handle<TermConnectHandler>>,
    connect_tx: mpsc::UnboundedSender<ConnectEvent>,
    connect_rx: mpsc::UnboundedReceiver<ConnectEvent>,
}

impl App {
    pub fn new() -> Result<Self> {
        Self::at(std::env::current_dir()?)
    }

    fn at(path: PathBuf) -> Result<Self> {
        let (connect_tx, connect_rx) = mpsc::unbounded_channel();

        Ok(Self {
            should_quit: false,
            screen: Screen::Files,
            active_panel: ActivePanel::Local,
            local: PanelState::new(path)?,
            dialog: None,
            pending_action: None,
            pending_password: None,
            status: None,
            connections: Vec::new(),
            connections_cursor: 0,
            connection_status: ConnectionStatus::Disconnected,
            connection_handle: None,
            connect_tx,
            connect_rx,
        })
    }

    pub async fn run(&mut self, terminal: &mut ratatui::Terminal<Backend>) -> Result<()> {
        let mut events = EventStream::new();

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                event = events.next() => {
                    if let Some(event) = event
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
                Some(connect_event) = self.connect_rx.recv() => {
                    self.apply_connect_event(connect_event);
                }
            }
        }

        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let (title_area, main_area, status_area) = layout::split_frame(frame.area());

        self.render_title(frame, title_area);

        match self.screen {
            Screen::Files => self.render_files(frame, main_area),
            Screen::Connections => self.render_connections(frame, main_area),
        }

        self.render_status(frame, status_area);

        if let Some(dialog) = &self.dialog {
            dialog.render(frame, frame.area());
        }
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let status_text = match &self.connection_status {
            ConnectionStatus::Disconnected => "Not connected".to_string(),
            ConnectionStatus::Connecting(name) => format!("Connecting to {name}\u{2026}"),
            ConnectionStatus::Connected(name) => format!("{name} \u{2014} SSH: Connected"),
            ConnectionStatus::Failed(message) => format!("Connection failed: {message}"),
        };

        let text = format!(
            "TermConnect{:>width$}",
            status_text,
            width = status_text.len() + 4
        );
        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_files(&self, frame: &mut Frame, area: Rect) {
        let (local_area, remote_area) = layout::split_panels(area);

        panels::render_panel(
            frame,
            local_area,
            "LOCAL",
            self.active_panel == ActivePanel::Local,
            &self.local,
        );

        let remote_title = match &self.connection_status {
            ConnectionStatus::Connected(name) => format!("REMOTE {name}"),
            ConnectionStatus::Connecting(name) => format!("REMOTE (connecting to {name}\u{2026})"),
            _ => "REMOTE".to_string(),
        };
        panels::render_placeholder(
            frame,
            remote_area,
            &remote_title,
            self.active_panel == ActivePanel::Remote,
        );
    }

    fn render_connections(&self, frame: &mut Frame, area: Rect) {
        let active_name = match &self.connection_status {
            ConnectionStatus::Connected(name) => Some(name.as_str()),
            _ => None,
        };
        connections_list::render_connections_list(
            frame,
            area,
            &self.connections,
            self.connections_cursor,
            active_name,
        );
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
            Action::OpenConnections => self.open_connections_screen(),
            Action::Back => self.screen = Screen::Files,
            Action::Up | Action::Down | Action::ToggleSelect | Action::Open | Action::Refresh => {
                self.apply_screen_action(action);
            }
            Action::Noop => {}
        }
    }

    fn apply_screen_action(&mut self, action: Action) {
        match self.screen {
            Screen::Files => self.apply_panel_action(action),
            Screen::Connections => self.apply_connections_action(action),
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

    fn apply_connections_action(&mut self, action: Action) {
        match action {
            Action::Up => {
                self.connections_cursor = self.connections_cursor.saturating_sub(1);
            }
            Action::Down if self.connections_cursor + 1 < self.connections.len() => {
                self.connections_cursor += 1;
            }
            Action::Down => {}
            Action::Open => self.connect_to_selected(),
            Action::Refresh => self.open_connections_screen(),
            _ => {}
        }
    }

    fn open_connections_screen(&mut self) {
        self.screen = Screen::Connections;

        match connection::list_all() {
            Ok(entries) => {
                self.connections = entries;
                self.connections_cursor = self
                    .connections_cursor
                    .min(self.connections.len().saturating_sub(1));
                self.status = None;
            }
            Err(err) => self.status = Some(err.to_string()),
        }
    }

    fn connect_to_selected(&mut self) {
        let Some(entry) = self.connections.get(self.connections_cursor).cloned() else {
            return;
        };

        if matches!(&self.connection_status, ConnectionStatus::Connected(name) if *name == entry.name)
        {
            self.connection_handle = None;
            self.connection_status = ConnectionStatus::Disconnected;
            return;
        }

        self.connection_status = ConnectionStatus::Connecting(entry.name.clone());
        let tx = self.connect_tx.clone();
        tokio::spawn(run_connect(entry, tx));
    }

    fn apply_connect_event(&mut self, event: ConnectEvent) {
        match event {
            ConnectEvent::Connected { name, handle } => {
                self.connection_handle = Some(handle);
                self.connection_status = ConnectionStatus::Connected(name);
                self.status = None;
            }
            ConnectEvent::NeedsPassword {
                name,
                username,
                respond_to,
            } => {
                self.pending_password = Some(respond_to);
                self.dialog = Some(Dialog::TextInput(TextInputDialog::new_masked(format!(
                    "Password for {username}@{name}"
                ))));
                self.pending_action = Some(PendingAction::SubmitPassword);
            }
            ConnectEvent::Failed { message } => {
                self.connection_status = ConnectionStatus::Failed(message.clone());
                self.status = Some(message);
            }
        }
    }

    fn open_mkdir_dialog(&mut self) {
        if self.screen != Screen::Files || self.active_panel != ActivePanel::Local {
            return;
        }

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new(
            "New directory name",
            "",
        )));
        self.pending_action = Some(PendingAction::Mkdir);
    }

    fn open_rename_dialog(&mut self) {
        if self.screen != Screen::Files || self.active_panel != ActivePanel::Local {
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
        if self.screen != Screen::Files || self.active_panel != ActivePanel::Local {
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
                    let result = self.local.delete_targets();
                    self.set_status(result);
                }
            }
            DialogOutcome::Submitted(value) => {
                self.dialog = None;
                match self.pending_action.take() {
                    Some(PendingAction::Mkdir) => {
                        let result = self.local.create_directory(&value);
                        self.set_status(result);
                    }
                    Some(PendingAction::Rename) => {
                        let result = self.local.rename_current(&value);
                        self.set_status(result);
                    }
                    Some(PendingAction::SubmitPassword) => {
                        if let Some(sender) = self.pending_password.take() {
                            let _ = sender.send(value);
                        }
                    }
                    Some(PendingAction::Delete) | None => {}
                }
            }
        }
    }

    fn set_status(&mut self, result: Result<()>) {
        self.status = result.err().map(|err| err.to_string());
    }
}

/// Runs a full connection attempt in the background: dial, verify the host
/// key, then authenticate in the roadmap's priority order (agent, key
/// file, password), reporting progress back over `tx` so the UI never
/// blocks on network I/O. A password prompt is requested via a one-shot
/// round-trip embedded in [`ConnectEvent::NeedsPassword`].
async fn run_connect(entry: ConnectionEntry, tx: mpsc::UnboundedSender<ConnectEvent>) {
    let mut handle = match connection::client::connect(&entry.host, entry.port).await {
        Ok(handle) => handle,
        Err(err) => {
            let _ = tx.send(ConnectEvent::Failed {
                message: err.to_string(),
            });
            return;
        }
    };

    match connection::client::authenticate_non_interactive(&mut handle, &entry).await {
        Ok(true) => {
            let _ = tx.send(ConnectEvent::Connected {
                name: entry.name.clone(),
                handle,
            });
            return;
        }
        Ok(false) => {}
        Err(err) => {
            let _ = tx.send(ConnectEvent::Failed {
                message: err.to_string(),
            });
            return;
        }
    }

    let (respond_to, password_rx) = oneshot::channel();
    let request = ConnectEvent::NeedsPassword {
        name: entry.name.clone(),
        username: entry.username.clone(),
        respond_to,
    };
    if tx.send(request).is_err() {
        return;
    }

    let Ok(password) = password_rx.await else {
        let _ = tx.send(ConnectEvent::Failed {
            message: "Connection cancelled".to_string(),
        });
        return;
    };

    let event =
        match connection::client::authenticate_password(&mut handle, &entry.username, &password)
            .await
        {
            Ok(true) => ConnectEvent::Connected {
                name: entry.name.clone(),
                handle,
            },
            Ok(false) => ConnectEvent::Failed {
                message: format!("Authentication failed for {}", entry.name),
            },
            Err(err) => ConnectEvent::Failed {
                message: err.to_string(),
            },
        };

    let _ = tx.send(event);
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

    #[test]
    fn open_connections_switches_screen_and_loads_entries() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::OpenConnections);
        assert_eq!(app.screen, Screen::Connections);
    }

    #[test]
    fn back_action_returns_to_files_screen() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::OpenConnections);
        app.apply_action(Action::Back);
        assert_eq!(app.screen, Screen::Files);
    }

    #[test]
    fn connections_cursor_moves_within_bounds() {
        let (_dir, mut app) = app_in_temp_dir();
        app.screen = Screen::Connections;
        app.connections = vec![
            ConnectionEntry {
                name: "a".to_string(),
                host: "a.example.com".to_string(),
                port: 22,
                username: "user".to_string(),
                identity_file: None,
            },
            ConnectionEntry {
                name: "b".to_string(),
                host: "b.example.com".to_string(),
                port: 22,
                username: "user".to_string(),
                identity_file: None,
            },
        ];

        app.apply_action(Action::Up);
        assert_eq!(app.connections_cursor, 0);

        app.apply_action(Action::Down);
        assert_eq!(app.connections_cursor, 1);

        app.apply_action(Action::Down);
        assert_eq!(app.connections_cursor, 1);
    }

    #[tokio::test]
    async fn connecting_to_an_unreachable_host_reports_failure() {
        let (_dir, mut app) = app_in_temp_dir();
        app.screen = Screen::Connections;
        app.connections = vec![ConnectionEntry {
            name: "unreachable".to_string(),
            host: "127.0.0.1".to_string(),
            port: 1, // nothing listens on port 1
            username: "user".to_string(),
            identity_file: None,
        }];

        app.connect_to_selected();
        assert_eq!(
            app.connection_status,
            ConnectionStatus::Connecting("unreachable".to_string())
        );

        let event = app.connect_rx.recv().await.unwrap();
        app.apply_connect_event(event);

        match app.connection_status {
            ConnectionStatus::Failed(_) => {}
            ref other => panic!("expected Failed, got {other:?}"),
        }
    }
}
