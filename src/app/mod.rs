use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use russh_sftp::client::SftpSession;
use tokio::sync::{mpsc, oneshot};

use crate::config;
use crate::connection::client::TermConnectHandler;
use crate::connection::{self, ConnectionEntry};
use crate::errors;
use crate::filesystem::search::SearchEvent;
use crate::filesystem::{self, Entry};
use crate::terminal;
use crate::transfer::{self, Direction, JobStatus, TransferOutcome, TransferQueue};
use crate::tui::connections_list;
use crate::tui::dialog::{ConfirmDialog, Dialog, DialogOutcome, ListDialog, TextInputDialog};
use crate::tui::help;
use crate::tui::input::{self, Action};
use crate::tui::notifications::{Notifications, Severity};
use crate::tui::panels::{self, ActivePanel, PanelState};
use crate::tui::search_view;
use crate::tui::search_view::{SearchOutcome, SearchView};
use crate::tui::sort::{SortKey, SortOrder};
use crate::tui::{self, Backend, layout};

mod actions;
mod bookmarks;
mod connections;
mod dialogs;
mod render;
mod search;
mod transfers;

/// The file operation a dialog is currently collecting input/confirmation for.
enum PendingAction {
    Mkdir,
    Rename,
    Delete,
    AddBookmark,
    SubmitPassword,
}

/// Which top-level screen is currently shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Files,
    Connections,
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectionStatus {
    Disconnected,
    Connecting(String),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchTarget {
    Local,
    Remote,
}

struct SearchSession {
    view: SearchView,
    target: SearchTarget,
    cancel: Arc<AtomicBool>,
    /// Bumped on every pattern change; a debounced search checks this after
    /// its idle delay and bails out as a no-op if it's no longer current
    /// (see `restart_search`).
    generation: Arc<AtomicU64>,
}

/// Progress reported by a background connection attempt (see
/// [`run_connect`]), delivered back to the event loop over a channel so the
/// TUI never blocks on network I/O.
enum ConnectEvent {
    Connected {
        entry: ConnectionEntry,
        handle: russh::client::Handle<TermConnectHandler>,
        sftp: SftpSession,
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

/// The result of a background SFTP operation on the remote panel: every
/// remote action (navigate, mkdir, rename, delete, refresh) ends the same
/// way — either a fresh listing to show, or a failure message.
enum PanelEvent {
    Listed {
        session_id: u64,
        path: PathBuf,
        entries: Vec<Entry>,
    },
    Failed {
        session_id: u64,
        message: String,
    },
}

/// Progress reported by a running file transfer (see `run_transfer`).
enum TransferEvent {
    Progress { id: u64, transferred: u64 },
    Finished { id: u64, outcome: TransferOutcome },
    Failed { id: u64, message: String },
}

/// The parts of a connected session that can't live in `Sessions` itself
/// (see Task 22/23's interface notes): `Handle` isn't `Clone`, and neither
/// type can be constructed without a live connection, which would make
/// `Sessions`'s own tests need one too.
struct SessionResources {
    handle: Arc<russh::client::Handle<TermConnectHandler>>,
    sftp: Arc<SftpSession>,
}

pub struct App {
    should_quit: bool,
    screen: Screen,
    active_panel: ActivePanel,
    local: PanelState,
    sessions: connection::session::Sessions,
    session_resources: HashMap<u64, SessionResources>,
    dialog: Option<Dialog>,
    help_visible: bool,
    pending_action: Option<PendingAction>,
    pending_password: Option<oneshot::Sender<String>>,
    notifications: Notifications,
    connections: Vec<ConnectionEntry>,
    connections_cursor: usize,
    connection_status: ConnectionStatus,
    search: Option<SearchSession>,
    search_tx: mpsc::UnboundedSender<SearchEvent>,
    search_rx: mpsc::UnboundedReceiver<SearchEvent>,
    connect_tx: mpsc::UnboundedSender<ConnectEvent>,
    connect_rx: mpsc::UnboundedReceiver<ConnectEvent>,
    panel_tx: mpsc::UnboundedSender<PanelEvent>,
    panel_rx: mpsc::UnboundedReceiver<PanelEvent>,
    transfers: TransferQueue,
    active_transfer_cancel: Option<Arc<AtomicBool>>,
    transfer_tx: mpsc::UnboundedSender<TransferEvent>,
    transfer_rx: mpsc::UnboundedReceiver<TransferEvent>,
    key_bindings: input::KeyBindings,
    bookmarks: config::bookmarks::Bookmarks,
    bookmarks_path: Option<PathBuf>,
}

impl App {
    pub fn new() -> Result<Self> {
        let (settings, config_warnings) = config::load()?;
        let (key_bindings, key_warnings) = input::KeyBindings::from_overrides(&settings.keys);
        let (bookmarks, bookmark_warnings) = config::bookmarks::load()?;
        let bookmarks_path = config::bookmarks::bookmarks_path()?;

        let mut app = Self::at_with(
            std::env::current_dir()?,
            &settings.panel,
            key_bindings,
            bookmarks,
            Some(bookmarks_path),
        )?;

        for warning in config_warnings {
            app.notifications.push(Severity::Warning, warning.0);
        }
        for warning in key_warnings {
            app.notifications.push(Severity::Warning, warning);
        }
        for warning in bookmark_warnings {
            app.notifications.push(Severity::Warning, warning.0);
        }

        Ok(app)
    }

    fn at(path: PathBuf) -> Result<Self> {
        Self::at_with(
            path,
            &config::settings::PanelSettings::default(),
            input::KeyBindings::defaults(),
            config::bookmarks::Bookmarks::default(),
            None,
        )
    }

    fn at_with(
        path: PathBuf,
        panel_settings: &config::settings::PanelSettings,
        key_bindings: input::KeyBindings,
        bookmarks: config::bookmarks::Bookmarks,
        bookmarks_path: Option<PathBuf>,
    ) -> Result<Self> {
        let (connect_tx, connect_rx) = mpsc::unbounded_channel();
        let (panel_tx, panel_rx) = mpsc::unbounded_channel();
        let (transfer_tx, transfer_rx) = mpsc::unbounded_channel();
        let (search_tx, search_rx) = mpsc::unbounded_channel();

        let mut local = PanelState::new(path)?;
        local.set_show_hidden(panel_settings.show_hidden);
        local.set_sort_spec(parse_sort_spec(panel_settings));

        Ok(Self {
            should_quit: false,
            screen: Screen::Files,
            active_panel: ActivePanel::Local,
            local,
            sessions: connection::session::Sessions::new(),
            session_resources: HashMap::new(),
            dialog: None,
            help_visible: false,
            pending_action: None,
            pending_password: None,
            notifications: Notifications::default(),
            connections: Vec::new(),
            connections_cursor: 0,
            connection_status: ConnectionStatus::Disconnected,
            search: None,
            search_tx,
            search_rx,
            connect_tx,
            connect_rx,
            panel_tx,
            panel_rx,
            transfers: TransferQueue::new(),
            active_transfer_cancel: None,
            transfer_tx,
            transfer_rx,
            key_bindings,
            bookmarks,
            bookmarks_path,
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
                        if self.help_visible {
                            self.help_visible = false;
                        } else if self.dialog.is_some() {
                            self.apply_dialog_key(key);
                        } else if self.screen == Screen::Search {
                            self.apply_search_key(key);
                        } else {
                            let action = self.key_bindings.map_key(key);
                            if action == Action::OpenTerminal {
                                self.launch_ssh_terminal(terminal).await?;
                                // The alternate screen and raw mode were
                                // left and re-entered around `ssh`; a
                                // fresh EventStream avoids relying on the
                                // old one's internal state surviving that.
                                events = EventStream::new();
                            } else {
                                self.apply_action(action);
                            }
                        }
                    }
                }
                Some(connect_event) = self.connect_rx.recv() => {
                    self.apply_connect_event(connect_event);
                }
                Some(panel_event) = self.panel_rx.recv() => {
                    self.apply_panel_event(panel_event);
                }
                Some(transfer_event) = self.transfer_rx.recv() => {
                    self.apply_transfer_event(transfer_event);
                }
                Some(search_event) = self.search_rx.recv() => {
                    self.apply_search_event(search_event);
                }
                _ = sleep_until_or_pending(self.notifications.next_wake()) => {
                    self.notifications.expire(Instant::now());
                }
            }
        }

        Ok(())
    }

    /// Reports the outcome of a synchronous local action. Success is a
    /// no-op — it no longer clears whatever notification is currently
    /// showing, since an `Error` notification must persist until the user
    /// acknowledges it (`Esc`), not get silently overwritten by the next
    /// unrelated success.
    fn set_status(&mut self, result: Result<()>) {
        if let Err(err) = result {
            self.notifications.push(Severity::Error, err.to_string());
        }
    }
}

/// SFTP paths are always POSIX-style strings; since TermConnect targets
/// Linux only, a `PathBuf`'s own `Display` already produces exactly that.
fn path_to_remote_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// How long a search waits, idle, before actually dispatching — coalesces a
/// burst of pattern-changing keystrokes into a single search per pause.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// Converts saved panel settings into a `SortSpec` — the one place allowed
/// to know about both `config::settings::PanelSettings`'s strings and
/// `tui::sort::SortSpec`'s enums.
fn parse_sort_spec(panel: &config::settings::PanelSettings) -> crate::tui::sort::SortSpec {
    let key = match panel.sort_key.as_str() {
        "size" => SortKey::Size,
        _ => SortKey::Name,
    };
    let order = match panel.sort_order.as_str() {
        "descending" => SortOrder::Descending,
        _ => SortOrder::Ascending,
    };
    crate::tui::sort::SortSpec { key, order }
}

/// Resolves at `deadline`, or never resolves if there's nothing to wait
/// for — so a `tokio::select!` branch built from this doesn't wake the
/// idle event loop on a timer it doesn't need.
async fn sleep_until_or_pending(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => std::future::pending().await,
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

    fn sample_connection_entry() -> ConnectionEntry {
        ConnectionEntry {
            name: "test".to_string(),
            host: "test.example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            identity_file: None,
        }
    }

    #[test]
    fn at_with_applies_panel_settings_to_the_local_panel() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".hidden"), b"x").unwrap();
        let settings = config::settings::PanelSettings {
            show_hidden: true,
            sort_key: "name".to_string(),
            sort_order: "ascending".to_string(),
        };

        let app = App::at_with(
            dir.path().to_path_buf(),
            &settings,
            input::KeyBindings::defaults(),
            config::bookmarks::Bookmarks::default(),
            None,
        )
        .unwrap();

        assert!(app.local.show_hidden());
    }

    #[test]
    fn key_bindings_from_config_are_used_for_key_mapping() {
        let (_dir, mut app) = app_in_temp_dir();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("quit".to_string(), "ctrl+q".to_string());
        let (bindings, _) = input::KeyBindings::from_overrides(&overrides);
        app.key_bindings = bindings;

        let event = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };

        assert_eq!(app.key_bindings.map_key(event), Action::Quit);
    }

    #[test]
    fn quit_action_sets_should_quit() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn help_action_shows_the_overlay() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::Help);
        assert!(app.help_visible);
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
    fn panel_actions_are_ignored_when_remote_panel_has_no_listing_yet() {
        let (dir, mut app) = app_in_temp_dir();
        fs::create_dir(dir.path().join("child")).unwrap();
        app.apply_action(Action::SwitchPanel);

        let cursor_before = app.local.cursor;
        app.apply_action(Action::Down);

        assert_eq!(app.local.cursor, cursor_before);
    }

    #[test]
    fn remote_panel_navigation_works_once_a_listing_exists() {
        let (_dir, mut app) = app_in_temp_dir();
        let panel = PanelState::from_listing(
            PathBuf::from("/home/user"),
            vec![Entry {
                name: "child".to_string(),
                path: PathBuf::from("/home/user/child"),
                is_dir: true,
                size: 0,
                permissions: None,
            }],
        );
        app.sessions.insert(sample_connection_entry(), panel);
        app.active_panel = ActivePanel::Remote;

        app.apply_action(Action::Down);

        assert_eq!(app.sessions.active().unwrap().panel.cursor, 1);
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

        assert!(app.notifications.current().is_some());
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

    #[test]
    fn panel_event_listed_updates_the_matching_sessions_panel() {
        let (_dir, mut app) = app_in_temp_dir();
        let id = app.sessions.insert(
            sample_connection_entry(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );

        app.apply_panel_event(PanelEvent::Listed {
            session_id: id,
            path: PathBuf::from("/home/user"),
            entries: Vec::new(),
        });

        assert_eq!(
            app.sessions.active().unwrap().panel.path(),
            std::path::Path::new("/home/user")
        );
    }

    #[test]
    fn panel_event_listed_for_a_vanished_session_is_dropped() {
        let (_dir, mut app) = app_in_temp_dir();

        app.apply_panel_event(PanelEvent::Listed {
            session_id: 999,
            path: PathBuf::from("/x"),
            entries: Vec::new(),
        });

        assert!(app.sessions.is_empty());
    }

    #[test]
    fn panel_event_failed_shows_a_notification_when_the_session_still_exists() {
        let (_dir, mut app) = app_in_temp_dir();
        let id = app.sessions.insert(
            sample_connection_entry(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );

        app.apply_panel_event(PanelEvent::Failed {
            session_id: id,
            message: "boom".to_string(),
        });

        assert_eq!(app.notifications.current().unwrap().message, "boom");
    }

    #[test]
    fn panel_event_failed_for_a_vanished_session_is_dropped() {
        let (_dir, mut app) = app_in_temp_dir();

        app.apply_panel_event(PanelEvent::Failed {
            session_id: 999,
            message: "boom".to_string(),
        });

        assert!(app.notifications.current().is_none());
    }

    #[test]
    fn cycle_session_action_is_a_silent_no_op_with_no_sessions() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::CycleSession);
        assert!(app.sessions.is_empty());
    }

    #[test]
    fn cycle_session_action_advances_the_active_session() {
        let (_dir, mut app) = app_in_temp_dir();
        let a = app.sessions.insert(
            sample_connection_entry(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );
        let mut second_entry = sample_connection_entry();
        second_entry.name = "other".to_string();
        let b = app.sessions.insert(
            second_entry,
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );
        assert_eq!(app.sessions.active().unwrap().id, b);

        app.apply_action(Action::CycleSession);

        assert_eq!(app.sessions.active().unwrap().id, a);
    }

    #[test]
    fn connecting_to_an_already_connected_host_switches_instead_of_reconnecting() {
        let (_dir, mut app) = app_in_temp_dir();
        let entry = sample_connection_entry();
        app.sessions.insert(
            entry.clone(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );
        app.connections = vec![entry];
        app.connections_cursor = 0;

        app.connect_to_selected();

        // still exactly one session — no reconnect attempt was spawned
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.connection_status, ConnectionStatus::Disconnected);
    }

    #[test]
    fn delete_on_the_connections_screen_disconnects_the_selected_session() {
        let (_dir, mut app) = app_in_temp_dir();
        let entry = sample_connection_entry();
        app.sessions.insert(
            entry.clone(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );
        app.connections = vec![entry];
        app.connections_cursor = 0;
        app.screen = Screen::Connections;

        assert_eq!(app.sessions.len(), 1);

        app.apply_action(Action::Delete);

        // The session (and any resources held for it) are gone — this is
        // the only way a connected session can ever be closed, since
        // `Sessions`/`SessionResources` can't be constructed with a live
        // handle in a unit test (see `SessionResources`'s doc comment), so
        // `session_resources` itself starts and stays empty here; the bug
        // this guards against is `Action::Delete` never reaching
        // `disconnect_selected` at all (it was intercepted earlier by the
        // mkdir/delete-dialog arm regardless of screen).
        assert!(app.sessions.is_empty());
        assert!(app.session_resources.is_empty());
    }

    #[test]
    fn disconnect_selected_reports_a_confirmation_notification() {
        let (_dir, mut app) = app_in_temp_dir();
        let entry = sample_connection_entry();
        app.sessions.insert(
            entry.clone(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );
        app.connections = vec![entry];
        app.connections_cursor = 0;
        app.screen = Screen::Connections;

        app.apply_action(Action::Delete);

        let messages: Vec<String> = std::iter::from_fn(|| {
            let message = app.notifications.current().map(|n| n.message.clone());
            if message.is_some() {
                app.notifications.dismiss_current();
            }
            message
        })
        .collect();

        assert!(
            messages.iter().any(|m| m.contains("Disconnected from")),
            "expected a disconnect confirmation notification, got {messages:?}"
        );
    }

    #[test]
    fn disconnecting_a_session_fails_its_queued_jobs_with_one_aggregated_notification() {
        let (_dir, mut app) = app_in_temp_dir();
        let entry = sample_connection_entry();
        let id = app.sessions.insert(
            entry.clone(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );
        app.connections = vec![entry];
        app.connections_cursor = 0;
        app.screen = Screen::Connections;

        let job_a = app.transfers.enqueue(
            id,
            Direction::Upload,
            PathBuf::from("/local/a.txt"),
            "/remote/a.txt".to_string(),
            "a.txt".to_string(),
            10,
        );
        let job_b = app.transfers.enqueue(
            id,
            Direction::Upload,
            PathBuf::from("/local/b.txt"),
            "/remote/b.txt".to_string(),
            "b.txt".to_string(),
            10,
        );
        // A job for a different session should be untouched.
        let other_session_job = app.transfers.enqueue(
            999,
            Direction::Upload,
            PathBuf::from("/local/c.txt"),
            "/remote/c.txt".to_string(),
            "c.txt".to_string(),
            10,
        );

        app.disconnect_selected();

        assert!(matches!(
            app.transfers.get(job_a).unwrap().status,
            JobStatus::Failed(_)
        ));
        assert!(matches!(
            app.transfers.get(job_b).unwrap().status,
            JobStatus::Failed(_)
        ));
        assert_eq!(
            app.transfers.get(other_session_job).unwrap().status,
            JobStatus::Queued
        );

        // Collect every notification pushed, in order.
        let messages: Vec<String> = std::iter::from_fn(|| {
            let message = app.notifications.current().map(|n| n.message.clone());
            if message.is_some() {
                app.notifications.dismiss_current();
            }
            message
        })
        .collect();

        // Exactly one message aggregates both cancelled/failed transfers —
        // not one notification per job — plus the disconnect confirmation.
        let transfer_messages: Vec<&String> = messages
            .iter()
            .filter(|m| m.contains("transfer") && m.contains("disconnected"))
            .collect();
        assert_eq!(
            transfer_messages.len(),
            1,
            "expected exactly one aggregated transfer notification, got {messages:?}"
        );
        assert!(transfer_messages[0].contains("2"));
    }

    #[test]
    fn delete_on_the_files_screen_still_opens_the_delete_dialog() {
        let (dir, mut app) = app_in_temp_dir();
        fs::write(dir.path().join("doomed.txt"), b"content").unwrap();
        app.local.refresh().unwrap();
        app.local.cursor = app.local.rows().len() - 1;

        app.apply_action(Action::Delete);

        assert!(app.dialog.is_some());
    }

    #[test]
    fn maybe_start_next_transfer_notifies_when_the_jobs_session_has_disconnected() {
        let (_dir, mut app) = app_in_temp_dir();
        // No session/session_resources entry for `999` exists — simulates a
        // job whose session disconnected before its turn came up.
        app.transfers.enqueue(
            999,
            Direction::Upload,
            PathBuf::from("/local/file.txt"),
            "/remote/file.txt".to_string(),
            "file.txt".to_string(),
            100,
        );

        app.maybe_start_next_transfer();

        let notification = app.notifications.current().unwrap();
        assert_eq!(notification.severity, Severity::Error);
        assert!(notification.message.contains("file.txt"));
        assert!(notification.message.contains("disconnected"));
    }

    #[test]
    fn failed_status_does_not_clobber_the_title_when_a_session_is_active() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (_dir, mut app) = app_in_temp_dir();
        app.sessions.insert(
            sample_connection_entry(),
            PanelState::from_listing(PathBuf::from("/"), Vec::new()),
        );
        app.connection_status = ConnectionStatus::Failed("boom".to_string());

        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| app.render_title(frame, frame.area()))
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("SSH: Connected"));
        assert!(!content.contains("Connection failed"));
    }

    #[test]
    fn failed_status_still_shows_when_there_is_no_active_session() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (_dir, mut app) = app_in_temp_dir();
        app.connection_status = ConnectionStatus::Failed("boom".to_string());

        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| app.render_title(frame, frame.area()))
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("Connection failed"));
    }

    #[test]
    fn toggle_hidden_action_reveals_dotfiles_in_the_local_panel() {
        let (dir, mut app) = app_in_temp_dir();
        fs::write(dir.path().join(".secret"), b"x").unwrap();
        app.local.refresh().unwrap();
        let before = app.local.rows().len();

        app.apply_action(Action::ToggleHidden);

        assert_eq!(app.local.rows().len(), before + 1);
    }

    #[test]
    fn cycle_sort_action_changes_the_local_panel_sort_spec() {
        let (_dir, mut app) = app_in_temp_dir();
        let before = app.local.sort_spec();

        app.apply_action(Action::CycleSort);

        assert_ne!(app.local.sort_spec(), before);
    }

    #[test]
    fn set_status_pushes_an_error_notification_on_failure() {
        let (_dir, mut app) = app_in_temp_dir();

        app.set_status(Err(anyhow::anyhow!("boom")));

        let current = app.notifications.current().unwrap();
        assert_eq!(current.message, "boom");
        assert_eq!(current.severity, Severity::Error);
    }

    #[test]
    fn set_status_does_nothing_on_success() {
        let (_dir, mut app) = app_in_temp_dir();

        app.set_status(Ok(()));

        assert!(app.notifications.current().is_none());
    }

    #[test]
    fn back_action_dismisses_an_error_notification_before_changing_screens() {
        let (_dir, mut app) = app_in_temp_dir();
        app.screen = Screen::Connections;
        app.notifications.push(Severity::Error, "oops");

        app.apply_action(Action::Back);

        assert!(app.notifications.current().is_none());
        assert_eq!(app.screen, Screen::Connections);
    }

    #[test]
    fn back_action_returns_to_files_screen_when_there_is_no_error() {
        let (_dir, mut app) = app_in_temp_dir();
        app.screen = Screen::Connections;

        app.apply_action(Action::Back);

        assert_eq!(app.screen, Screen::Files);
    }

    #[test]
    fn bookmark_here_action_opens_a_text_input_dialog_prefilled_with_the_directory_name() {
        let (dir, mut app) = app_in_temp_dir();

        app.apply_action(Action::BookmarkHere);

        match app.dialog {
            Some(Dialog::TextInput(ref d)) => {
                assert_eq!(d.value, dir.path().file_name().unwrap().to_str().unwrap());
            }
            _ => panic!("expected a text input dialog"),
        }
    }

    #[test]
    fn submitting_the_bookmark_dialog_adds_a_local_bookmark() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::BookmarkHere);

        app.apply_dialog_key(key(KeyCode::Char('x')));
        app.apply_dialog_key(key(KeyCode::Enter));

        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.bookmarks.get(0).unwrap().host, None);
    }

    #[test]
    fn open_bookmarks_action_lists_saved_bookmarks() {
        let (dir, mut app) = app_in_temp_dir();
        app.bookmarks.add(config::bookmarks::Bookmark {
            label: "here".to_string(),
            path: dir.path().to_path_buf(),
            host: None,
        });

        app.apply_action(Action::OpenBookmarks);

        match app.dialog {
            Some(Dialog::List(ref d)) => assert_eq!(d.items.len(), 1),
            _ => panic!("expected a list dialog"),
        }
    }

    #[test]
    fn selecting_a_local_bookmark_navigates_the_local_panel() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        let mut app = App::at(dir.path().to_path_buf()).unwrap();
        app.bookmarks.add(config::bookmarks::Bookmark {
            label: "child".to_string(),
            path: child.clone(),
            host: None,
        });
        app.apply_action(Action::OpenBookmarks);

        app.apply_dialog_key(key(KeyCode::Enter));

        assert_eq!(app.local.path(), child);
    }

    #[test]
    fn selecting_a_remote_bookmark_without_a_connection_warns_instead_of_navigating() {
        let (_dir, mut app) = app_in_temp_dir();
        app.bookmarks.add(config::bookmarks::Bookmark {
            label: "prod etc".to_string(),
            path: PathBuf::from("/etc"),
            host: Some("production".to_string()),
        });
        app.apply_action(Action::OpenBookmarks);

        app.apply_dialog_key(key(KeyCode::Enter));

        assert_eq!(
            app.notifications.current().unwrap().message,
            "Connect to production first"
        );
    }

    #[test]
    fn removing_a_bookmark_deletes_it_from_the_list() {
        let (_dir, mut app) = app_in_temp_dir();
        app.bookmarks.add(config::bookmarks::Bookmark {
            label: "a".to_string(),
            path: PathBuf::from("/a"),
            host: None,
        });
        app.apply_action(Action::OpenBookmarks);

        app.apply_dialog_key(key(KeyCode::F(8)));

        assert!(app.bookmarks.is_empty());
    }

    #[test]
    fn open_search_action_switches_to_the_search_screen_for_the_local_panel() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::OpenSearch);
        assert_eq!(app.screen, Screen::Search);
    }

    #[test]
    fn open_search_action_warns_when_remote_is_focused_without_a_connection() {
        let (_dir, mut app) = app_in_temp_dir();
        app.active_panel = ActivePanel::Remote;

        app.apply_action(Action::OpenSearch);

        assert_eq!(app.screen, Screen::Files);
        assert!(app.notifications.current().is_some());
    }

    #[test]
    fn esc_in_search_closes_the_screen() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_action(Action::OpenSearch);

        app.apply_search_key(key(KeyCode::Esc));

        assert_eq!(app.screen, Screen::Files);
        assert!(app.search.is_none());
    }

    #[tokio::test]
    async fn typing_a_pattern_streams_matching_results_back_into_the_search_view() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("target.log"), b"x").unwrap();
        let mut app = App::at(dir.path().to_path_buf()).unwrap();

        app.apply_action(Action::OpenSearch);
        for c in "target".chars() {
            app.apply_search_key(key(KeyCode::Char(c)));
        }

        // Longer than `SEARCH_DEBOUNCE`, so the debounced search past the
        // last keystroke has actually dispatched and completed by the time
        // we drain `search_rx` below.
        tokio::time::sleep(SEARCH_DEBOUNCE + std::time::Duration::from_millis(200)).await;
        while let Ok(event) = app.search_rx.try_recv() {
            app.apply_search_event(event);
        }

        assert!(
            app.search
                .unwrap()
                .view
                .results
                .iter()
                .any(|entry| entry.name == "target.log")
        );
    }

    #[tokio::test]
    async fn rapid_pattern_changes_dispatch_only_one_search_for_the_final_pattern() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("aaa.log"), b"x").unwrap();
        fs::write(dir.path().join("bbb.log"), b"x").unwrap();
        let mut app = App::at(dir.path().to_path_buf()).unwrap();

        app.apply_action(Action::OpenSearch);
        // Simulate rapid typing: each keystroke calls `restart_search`
        // (bumping the search generation and cancelling the previous
        // debounce/search), all faster than `SEARCH_DEBOUNCE`, so only the
        // last one should ever actually dispatch a search.
        for c in "aaa".chars() {
            app.apply_search_key(key(KeyCode::Char(c)));
        }

        tokio::time::sleep(SEARCH_DEBOUNCE + std::time::Duration::from_millis(200)).await;
        let mut done_count = 0;
        while let Ok(event) = app.search_rx.try_recv() {
            if matches!(event, SearchEvent::Done { .. }) {
                done_count += 1;
            }
            app.apply_search_event(event);
        }

        // Exactly one search actually ran (one `Done`), and it was for the
        // final pattern "aaa" — not one per keystroke ("a", "aa", "aaa").
        assert_eq!(done_count, 1, "expected exactly one dispatched search");
        let session = app.search.unwrap();
        assert!(
            session
                .view
                .results
                .iter()
                .any(|entry| entry.name == "aaa.log")
        );
        assert!(
            !session
                .view
                .results
                .iter()
                .any(|entry| entry.name == "bbb.log")
        );
    }
}
