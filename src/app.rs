use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

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
use crate::filesystem::search::{self, SearchEvent};
use crate::filesystem::{self, Entry};
use crate::terminal;
use crate::transfer::{self, Direction, JobStatus, TransferOutcome, TransferQueue};
use crate::tui::input::{self, Action};
use crate::tui::notifications::{Notifications, Severity};
use crate::tui::panels::{self, ActivePanel, PanelState};
use crate::tui::sort::{SortKey, SortOrder};
use crate::tui::widgets::connections_list;
use crate::tui::widgets::dialog::{
    ConfirmDialog, Dialog, DialogOutcome, ListDialog, TextInputDialog,
};
use crate::tui::widgets::help;
use crate::tui::widgets::search_view;
use crate::tui::widgets::search_view::{SearchOutcome, SearchView};
use crate::tui::{self, Backend, layout};

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

    fn render(&self, frame: &mut Frame) {
        let (title_area, main_area, status_area) = layout::split_frame(frame.area());

        self.render_title(frame, title_area);

        match self.screen {
            Screen::Files => self.render_files(frame, main_area),
            Screen::Connections => self.render_connections(frame, main_area),
            Screen::Search => self.render_search(frame, main_area),
        }

        self.render_status(frame, status_area);

        if let Some(dialog) = &self.dialog {
            dialog.render(frame, frame.area());
        }

        if self.help_visible {
            help::render_help(frame, frame.area(), &self.key_bindings);
        }
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let status_text = match &self.connection_status {
            ConnectionStatus::Connecting(name) => format!("Connecting to {name}\u{2026}"),
            ConnectionStatus::Failed(message) if self.sessions.is_empty() => {
                format!("Connection failed: {message}")
            }
            _ => match self.sessions.active() {
                Some(session) => format!("{} \u{2014} SSH: Connected", session.entry.name),
                None => "Not connected".to_string(),
            },
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

        match self.sessions.active() {
            Some(session) => {
                if self.sessions.len() > 1 {
                    let (tabs_area, panel_area) = layout::split_remote_with_tabs(remote_area);
                    self.render_session_tabs(frame, tabs_area);
                    panels::render_panel(
                        frame,
                        panel_area,
                        "REMOTE",
                        self.active_panel == ActivePanel::Remote,
                        &session.panel,
                    );
                } else {
                    panels::render_panel(
                        frame,
                        remote_area,
                        "REMOTE",
                        self.active_panel == ActivePanel::Remote,
                        &session.panel,
                    );
                }
            }
            None => {
                let remote_title = match &self.connection_status {
                    ConnectionStatus::Connecting(name) => {
                        format!("REMOTE (connecting to {name}\u{2026})")
                    }
                    _ => "REMOTE".to_string(),
                };
                panels::render_placeholder(
                    frame,
                    remote_area,
                    &remote_title,
                    self.active_panel == ActivePanel::Remote,
                );
            }
        }
    }

    /// A one-line strip of session host names, the active one marked with
    /// `>` — plain text rather than a styled tab widget, matching the rest
    /// of the app's low-frills rendering.
    fn render_session_tabs(&self, frame: &mut Frame, area: Rect) {
        let active_id = self.sessions.active_id();
        let labels: Vec<String> = self
            .sessions
            .iter()
            .map(|session| {
                let marker = if Some(session.id) == active_id {
                    '>'
                } else {
                    ' '
                };
                format!("{marker}{}", session.entry.name)
            })
            .collect();
        frame.render_widget(Paragraph::new(labels.join("  ")), area);
    }

    fn render_connections(&self, frame: &mut Frame, area: Rect) {
        let active_name = self
            .sessions
            .active()
            .map(|session| session.entry.name.as_str());
        connections_list::render_connections_list(
            frame,
            area,
            &self.connections,
            self.connections_cursor,
            active_name,
        );
    }

    fn render_search(&self, frame: &mut Frame, area: Rect) {
        if let Some(session) = &self.search {
            search_view::render_search(frame, area, &session.view);
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let (text, style) = match self.notifications.current() {
            Some(notification) => (
                notification.message.clone(),
                notification_style(notification.severity),
            ),
            None => match self.transfers.active() {
                Some(job) => (self.transfer_status_text(job), Style::default()),
                None => (build_hint_text(&self.key_bindings), Style::default()),
            },
        };

        frame.render_widget(Paragraph::new(text).style(style), area);
    }

    fn transfer_status_text(&self, job: &transfer::TransferJob) -> String {
        let verb = match job.direction {
            Direction::Upload => "Uploading",
            Direction::Download => "Downloading",
        };
        let queued = self.transfers.queued_count();
        let suffix = if queued > 0 {
            format!(" ({queued} queued)")
        } else {
            String::new()
        };
        format!(
            "{verb} {}: {}%{suffix}",
            job.display_name,
            job.progress_percent()
        )
    }

    fn apply_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::SwitchPanel => self.active_panel.toggle(),
            Action::Mkdir => self.open_mkdir_dialog(),
            Action::Rename => self.open_rename_dialog(),
            Action::Delete => match self.screen {
                Screen::Connections => self.disconnect_selected(),
                _ => self.open_delete_dialog(),
            },
            Action::Copy => self.start_copy(),
            Action::CancelTransfer => self.cancel_active_transfer(),
            Action::OpenConnections => self.open_connections_screen(),
            Action::BookmarkHere => self.open_bookmark_add_dialog(),
            Action::OpenBookmarks => self.open_bookmarks_dialog(),
            Action::OpenSearch => self.open_search_screen(),
            Action::CycleSession => {
                if self.screen == Screen::Files {
                    self.sessions.cycle();
                }
            }
            Action::Help => self.help_visible = true,
            Action::Back => self.handle_back(),
            Action::Up
            | Action::Down
            | Action::ToggleSelect
            | Action::Open
            | Action::Refresh
            | Action::ToggleHidden
            | Action::CycleSort => {
                self.apply_screen_action(action);
            }
            // Handled specially in `run`, which has the `&mut Terminal`
            // this needs to suspend/resume the TUI around `ssh`.
            Action::OpenTerminal => {}
            Action::Noop => {}
        }
    }

    /// `Esc`: dismisses a persistent error notification first, if one is
    /// showing; otherwise falls back to its usual meaning of closing the
    /// dialog/returning to the Files screen.
    fn handle_back(&mut self) {
        let showing_error = matches!(
            self.notifications.current().map(|n| n.severity),
            Some(Severity::Error)
        );
        if showing_error {
            self.notifications.dismiss_current();
        } else {
            self.screen = Screen::Files;
        }
    }

    fn apply_screen_action(&mut self, action: Action) {
        match self.screen {
            Screen::Files => self.apply_panel_action(action),
            Screen::Connections => self.apply_connections_action(action),
            Screen::Search => {}
        }
    }

    /// Actions that operate on whichever panel is focused.
    fn apply_panel_action(&mut self, action: Action) {
        match self.active_panel {
            ActivePanel::Local => self.apply_local_panel_action(action),
            ActivePanel::Remote => self.apply_remote_panel_action(action),
        }
    }

    fn apply_local_panel_action(&mut self, action: Action) {
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
            Action::ToggleHidden => {
                self.local.toggle_hidden();
                Ok(())
            }
            Action::CycleSort => {
                self.local.cycle_sort();
                Ok(())
            }
            _ => Ok(()),
        };

        self.set_status(result);
    }

    /// Remote navigation/selection is instant (pure state), but anything
    /// that needs a fresh listing (`Open`, `Refresh`) has to go over the
    /// network, so it's dispatched to a background task instead of run
    /// inline — see `spawn_remote_list`.
    fn apply_remote_panel_action(&mut self, action: Action) {
        let target_path = {
            let Some(session) = self.sessions.active_mut() else {
                return;
            };
            match action {
                Action::Up => {
                    session.panel.move_cursor(-1);
                    return;
                }
                Action::Down => {
                    session.panel.move_cursor(1);
                    return;
                }
                Action::ToggleSelect => {
                    session.panel.toggle_selection();
                    return;
                }
                Action::ToggleHidden => {
                    session.panel.toggle_hidden();
                    return;
                }
                Action::CycleSort => {
                    session.panel.cycle_sort();
                    return;
                }
                Action::Open => session.panel.target_path_for_open(),
                Action::Refresh => Some(session.panel.path().to_path_buf()),
                _ => return,
            }
        };

        if let Some(path) = target_path {
            self.spawn_remote_list(path);
        }
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

    /// Hands the terminal over to the system `ssh` client, per the
    /// roadmap's Terminal Lifecycle: leave the alternate screen, let `ssh`
    /// take stdin/stdout/stderr, wait for it to exit, then reinitialize the
    /// TUI. Nothing else in the event loop runs while `ssh` has the
    /// terminal, by design — that's the whole point of the handover.
    async fn launch_ssh_terminal(
        &mut self,
        terminal: &mut ratatui::Terminal<Backend>,
    ) -> Result<()> {
        let Some(entry) = self.sessions.active().map(|session| session.entry.clone()) else {
            self.notifications
                .push(Severity::Warning, "Connect to a server first");
            return Ok(());
        };

        tui::restore()?;

        let ssh_result = tokio::task::spawn_blocking(move || terminal::ssh::run(&entry)).await;

        // `tui::init` builds a brand-new `Terminal` with an empty internal
        // buffer, so the next `draw` repaints everything on its own —
        // deliberately not calling `.clear()` here, since it queries the
        // cursor position via a DSR escape sequence that some terminals
        // (or terminals mid-handover right after `ssh` exits) may not
        // answer in time, which would turn a cosmetic no-op into a crash.
        *terminal = tui::init()?;

        match ssh_result {
            Ok(Ok(status)) if !status.success() => {
                self.notifications
                    .push(Severity::Error, format!("ssh exited with status {status}"));
            }
            Ok(Ok(_)) => {}
            Ok(Err(io_err)) => {
                self.notifications
                    .push(Severity::Error, format!("Failed to launch ssh: {io_err}"));
            }
            Err(join_err) => {
                self.notifications
                    .push(Severity::Error, format!("ssh task failed: {join_err}"));
            }
        }

        Ok(())
    }

    fn open_connections_screen(&mut self) {
        self.screen = Screen::Connections;

        match connection::list_all() {
            Ok(entries) => {
                self.connections = entries;
                self.connections_cursor = self
                    .connections_cursor
                    .min(self.connections.len().saturating_sub(1));
            }
            Err(err) => self.notifications.push(Severity::Error, err.to_string()),
        }
    }

    fn open_search_screen(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let target = match self.active_panel {
            ActivePanel::Local => SearchTarget::Local,
            ActivePanel::Remote => {
                if self.sessions.active().is_none() {
                    self.notifications
                        .push(Severity::Warning, "Connect to a remote server first");
                    return;
                }
                SearchTarget::Remote
            }
        };

        self.search = Some(SearchSession {
            view: SearchView::new(),
            target,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        self.screen = Screen::Search;
    }

    fn apply_search_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(session) = &self.search {
                session.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }

        let Some(session) = self.search.as_mut() else {
            return;
        };

        match session.view.handle_key(key) {
            SearchOutcome::Cancel => {
                if let Some(session) = self.search.take() {
                    session.cancel.store(true, Ordering::Relaxed);
                }
                self.screen = Screen::Files;
            }
            SearchOutcome::PatternChanged => self.restart_search(),
            SearchOutcome::Open => self.open_selected_search_result(),
            SearchOutcome::Pending => {}
        }
    }

    /// Cancels any in-flight search and starts a new one for the current
    /// pattern — called on every keystroke that changes the pattern, so
    /// results filter live as the user types.
    fn restart_search(&mut self) {
        let Some(session) = self.search.as_mut() else {
            return;
        };
        session.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        session.cancel = cancel.clone();
        session.view.start();

        let pattern = session.view.pattern.clone();
        if pattern.is_empty() {
            return;
        }
        // The search box is a plain substring filter, not a glob editor —
        // wrap the typed text so `glob_match`'s anchored matching (see
        // `filesystem::search`) behaves like "contains" instead of
        // requiring an exact filename match.
        let glob_pattern = format!("*{pattern}*");

        let tx = self.search_tx.clone();
        match session.target {
            SearchTarget::Local => {
                let root = self.local.path().to_path_buf();
                tokio::spawn(search::search_local(root, glob_pattern, tx, cancel));
            }
            SearchTarget::Remote => {
                let Some(session_id) = self.sessions.active_id() else {
                    return;
                };
                let Some(resources) = self.session_resources.get(&session_id) else {
                    return;
                };
                let sftp = resources.sftp.clone();
                let handle = resources.handle.clone();
                let root = self
                    .sessions
                    .active()
                    .map(|session| session.panel.path().to_path_buf())
                    .unwrap_or_default();
                let root_str = path_to_remote_string(&root);
                tokio::spawn(async move {
                    search::search_remote(&handle, &sftp, root_str, glob_pattern, tx, cancel).await;
                });
            }
        }
    }

    fn apply_search_event(&mut self, event: SearchEvent) {
        let Some(session) = self.search.as_mut() else {
            return;
        };
        match event {
            SearchEvent::Found(entry) => session.view.push_result(entry),
            SearchEvent::Done { truncated } => session.view.finish(truncated),
            SearchEvent::Failed(message) => {
                session.view.finish(false);
                self.notifications.push(Severity::Error, message);
            }
        }
    }

    /// Closes the search screen and navigates the target panel to the
    /// selected result's parent directory.
    fn open_selected_search_result(&mut self) {
        let Some(session) = self.search.as_ref() else {
            return;
        };
        let Some(entry) = session.view.selected_entry().cloned() else {
            return;
        };
        let target_panel = match session.target {
            SearchTarget::Local => ActivePanel::Local,
            SearchTarget::Remote => ActivePanel::Remote,
        };
        let parent = entry
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| entry.path.clone());

        if let Some(session) = self.search.take() {
            session.cancel.store(true, Ordering::Relaxed);
        }
        self.screen = Screen::Files;
        self.active_panel = target_panel;

        match target_panel {
            ActivePanel::Local => {
                let result = self.local.navigate_to(parent);
                self.set_status(result);
            }
            ActivePanel::Remote => self.spawn_remote_list(parent),
        }
    }

    fn connect_to_selected(&mut self) {
        let Some(entry) = self.connections.get(self.connections_cursor).cloned() else {
            return;
        };

        if let Some(session) = self.sessions.by_host(&entry.name) {
            self.sessions.activate(session.id);
            return;
        }

        self.connection_status = ConnectionStatus::Connecting(entry.name.clone());
        let tx = self.connect_tx.clone();
        tokio::spawn(run_connect(entry, tx));
    }

    /// Disconnects the connection under the connections-screen cursor, if
    /// it's connected — `F8`'s counterpart to `Enter`'s connect/switch.
    fn disconnect_selected(&mut self) {
        let Some(entry) = self.connections.get(self.connections_cursor) else {
            return;
        };
        let Some(id) = self.sessions.by_host(&entry.name).map(|session| session.id) else {
            return;
        };

        self.sessions.remove(id);
        self.session_resources.remove(&id);
    }

    fn apply_connect_event(&mut self, event: ConnectEvent) {
        match event {
            ConnectEvent::Connected {
                entry,
                handle,
                sftp,
            } => {
                self.connection_status = ConnectionStatus::Disconnected;
                let placeholder_panel = PanelState::from_listing(PathBuf::from("/"), Vec::new());
                let id = self.sessions.insert(entry, placeholder_panel);
                self.session_resources.insert(
                    id,
                    SessionResources {
                        handle: Arc::new(handle),
                        sftp: Arc::new(sftp),
                    },
                );
                self.spawn_initial_remote_listing(id);
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
                self.notifications.push(Severity::Error, message);
            }
        }
    }

    fn apply_panel_event(&mut self, event: PanelEvent) {
        match event {
            PanelEvent::Listed {
                session_id,
                path,
                entries,
            } => {
                if let Some(session) = self.sessions.by_id_mut(session_id) {
                    session.panel.replace_listing(path, entries);
                }
            }
            PanelEvent::Failed {
                session_id,
                message,
            } => {
                if self.sessions.by_id_mut(session_id).is_some() {
                    self.notifications.push(Severity::Error, message);
                } else {
                    tracing::debug!(
                        "dropping stale panel error for session {session_id}: {message}"
                    );
                }
            }
        }
    }

    fn spawn_initial_remote_listing(&mut self, session_id: u64) {
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            let home = match sftp.canonicalize(".").await {
                Ok(home) => home,
                Err(err) => {
                    tracing::debug!("{err:?}");
                    let err = anyhow::Error::from(err);
                    let _ = tx.send(PanelEvent::Failed {
                        session_id,
                        message: errors::user_message("Unable to list home directory", &err),
                    });
                    return;
                }
            };
            relist(&sftp, session_id, PathBuf::from(home), &tx).await;
        });
    }

    /// Refreshes the *active* session's panel — used for user-driven
    /// navigation/refresh, where "the remote panel" unambiguously means
    /// whichever session is focused.
    fn spawn_remote_list(&mut self, path: PathBuf) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        self.spawn_remote_list_for(session_id, path);
    }

    /// Refreshes a specific session's panel by id — used when the session
    /// that needs refreshing isn't necessarily the active one (a finished
    /// transfer targets whichever session it was queued against).
    fn spawn_remote_list_for(&mut self, session_id: u64, path: PathBuf) {
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();
        tokio::spawn(async move { relist(&sftp, session_id, path, &tx).await });
    }

    fn spawn_remote_mkdir(&mut self, name: String) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        let Some(session) = self.sessions.active() else {
            return;
        };
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let dir_path = session.panel.path().to_path_buf();
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            let target = path_to_remote_string(&dir_path.join(&name));
            if let Err(err) = filesystem::remote::create_directory(&sftp, &target).await {
                tracing::debug!("{err:?}");
                let _ = tx.send(PanelEvent::Failed {
                    session_id,
                    message: errors::user_message("Unable to create directory", &err),
                });
                return;
            }
            relist(&sftp, session_id, dir_path, &tx).await;
        });
    }

    fn spawn_remote_rename(&mut self, new_name: String) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        let Some(session) = self.sessions.active() else {
            return;
        };
        let Some(current_name) = session.panel.current_entry_name() else {
            return;
        };
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let dir_path = session.panel.path().to_path_buf();
        let from = path_to_remote_string(&dir_path.join(current_name));
        let to = path_to_remote_string(&dir_path.join(&new_name));
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            if let Err(err) = filesystem::remote::rename(&sftp, &from, &to).await {
                tracing::debug!("{err:?}");
                let _ = tx.send(PanelEvent::Failed {
                    session_id,
                    message: errors::user_message("Unable to rename", &err),
                });
                return;
            }
            relist(&sftp, session_id, dir_path, &tx).await;
        });
    }

    fn spawn_remote_delete(&mut self) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        let Some(session) = self.sessions.active() else {
            return;
        };
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let targets = session.panel.targets();
        let dir_path = session.panel.path().to_path_buf();
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            for target in targets {
                let target_str = path_to_remote_string(&target);
                if let Err(err) = filesystem::remote::delete(&sftp, &target_str).await {
                    tracing::debug!("{err:?}");
                    let _ = tx.send(PanelEvent::Failed {
                        session_id,
                        message: errors::user_message("Unable to delete", &err),
                    });
                    return;
                }
            }
            relist(&sftp, session_id, dir_path, &tx).await;
        });
    }

    /// Copies the focused panel's selection (or the entry under the
    /// cursor) to the other panel's current directory — upload if LOCAL is
    /// focused, download if REMOTE is focused. Direction and source/target
    /// panel follow the roadmap's rule: "determined by the active/source
    /// panel."
    fn start_copy(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        match self.active_panel {
            ActivePanel::Local => self.enqueue_uploads(),
            ActivePanel::Remote => self.enqueue_downloads(),
        }

        self.maybe_start_next_transfer();
    }

    fn enqueue_uploads(&mut self) {
        let Some(session) = self.sessions.active() else {
            self.notifications
                .push(Severity::Warning, "Connect to a remote server first");
            return;
        };
        let session_id = session.id;
        let remote_dir = session.panel.path().to_path_buf();
        let entries = self.local.target_entries();
        self.enqueue_transfers(session_id, Direction::Upload, entries, remote_dir);
    }

    fn enqueue_downloads(&mut self) {
        let Some(session) = self.sessions.active() else {
            return;
        };
        let session_id = session.id;
        let local_dir = self.local.path().to_path_buf();
        let entries = session.panel.target_entries();
        self.enqueue_transfers(session_id, Direction::Download, entries, local_dir);
    }

    fn enqueue_transfers(
        &mut self,
        session_id: u64,
        direction: Direction,
        entries: Vec<Entry>,
        dest_dir: PathBuf,
    ) {
        if entries.is_empty() {
            return;
        }

        let mut skipped_dirs = 0;
        for entry in entries {
            if entry.is_dir {
                skipped_dirs += 1;
                continue;
            }

            let (local_path, remote_path) = match direction {
                Direction::Upload => (
                    entry.path.clone(),
                    path_to_remote_string(&dest_dir.join(&entry.name)),
                ),
                Direction::Download => (
                    dest_dir.join(&entry.name),
                    path_to_remote_string(&entry.path),
                ),
            };

            self.transfers.enqueue(
                session_id,
                direction,
                local_path,
                remote_path,
                entry.name,
                entry.size,
            );
        }

        if skipped_dirs > 0 {
            let plural = if skipped_dirs == 1 { "y" } else { "ies" };
            self.notifications.push(
                Severity::Warning,
                format!("Copying directories isn't supported yet \u{2014} skipped {skipped_dirs} director{plural}"),
            );
        }
    }

    fn maybe_start_next_transfer(&mut self) {
        let Some(id) = self.transfers.next_to_run() else {
            return;
        };
        let Some(job) = self.transfers.get(id) else {
            return;
        };
        let session_id = job.session_id;
        let display_name = job.display_name.clone();

        let Some(resources) = self.session_resources.get(&session_id) else {
            if let Some(job) = self.transfers.get_mut(id) {
                job.status = JobStatus::Failed("session disconnected".to_string());
            }
            self.notifications.push(
                Severity::Error,
                format!("Transfer failed: {display_name} \u{2014} session disconnected"),
            );
            self.maybe_start_next_transfer();
            return;
        };
        let sftp = resources.sftp.clone();

        let Some(job) = self.transfers.get_mut(id) else {
            return;
        };
        job.status = JobStatus::InProgress;
        job.attempts += 1;
        let direction = job.direction;
        let local_path = job.local_path.clone();
        let remote_path = job.remote_path.clone();
        let display_name = job.display_name.clone();

        let cancel = Arc::new(AtomicBool::new(false));
        self.active_transfer_cancel = Some(cancel.clone());

        let tx = self.transfer_tx.clone();
        tokio::spawn(async move {
            let progress_tx = tx.clone();
            let result = transfer::run(
                direction,
                &local_path,
                &remote_path,
                &sftp,
                &cancel,
                move |transferred| {
                    let _ = progress_tx.send(TransferEvent::Progress { id, transferred });
                },
            )
            .await;

            let event = match result {
                Ok(outcome) => TransferEvent::Finished { id, outcome },
                Err(err) => {
                    tracing::debug!("{err:?}");
                    TransferEvent::Failed {
                        id,
                        message: errors::user_message(
                            format!("Transfer failed: {display_name}"),
                            &err,
                        ),
                    }
                }
            };
            let _ = tx.send(event);
        });
    }

    fn apply_transfer_event(&mut self, event: TransferEvent) {
        match event {
            TransferEvent::Progress { id, transferred } => {
                if let Some(job) = self.transfers.get_mut(id) {
                    job.transferred_bytes = transferred;
                }
            }
            TransferEvent::Finished { id, outcome } => {
                self.active_transfer_cancel = None;
                if let Some(job) = self.transfers.get_mut(id) {
                    job.status = match outcome {
                        TransferOutcome::Completed => JobStatus::Completed,
                        TransferOutcome::Cancelled => JobStatus::Cancelled,
                    };
                }
                self.refresh_transfer_destination(id);
                self.maybe_start_next_transfer();
            }
            TransferEvent::Failed { id, message } => {
                self.active_transfer_cancel = None;
                if let Some(job) = self.transfers.get_mut(id) {
                    job.status = JobStatus::Failed(message.clone());
                }
                if !self.transfers.retry_or_give_up(id) {
                    self.notifications.push(Severity::Error, message);
                }
                self.maybe_start_next_transfer();
            }
        }
    }

    /// Refreshes whichever panel just received a file, so the new listing
    /// is visible without a manual `Ctrl+R`.
    fn refresh_transfer_destination(&mut self, id: u64) {
        let Some(job) = self.transfers.get(id) else {
            return;
        };

        match job.direction {
            Direction::Upload => {
                let session_id = job.session_id;
                if let Some(session) = self.sessions.by_id_mut(session_id) {
                    let path = session.panel.path().to_path_buf();
                    self.spawn_remote_list_for(session_id, path);
                }
            }
            Direction::Download => {
                let _ = self.local.refresh();
            }
        }
    }

    fn cancel_active_transfer(&mut self) {
        if let Some(cancel) = &self.active_transfer_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn open_bookmark_add_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let default_label = self
            .active_panel_path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("bookmark")
            .to_string();

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new(
            "Bookmark name",
            default_label,
        )));
        self.pending_action = Some(PendingAction::AddBookmark);
    }

    fn active_panel_path(&self) -> PathBuf {
        match self.active_panel {
            ActivePanel::Local => self.local.path().to_path_buf(),
            ActivePanel::Remote => self
                .sessions
                .active()
                .map(|session| session.panel.path().to_path_buf())
                .unwrap_or_default(),
        }
    }

    fn add_bookmark(&mut self, label: String) {
        let host = match self.active_panel {
            ActivePanel::Local => None,
            ActivePanel::Remote => match self.sessions.active() {
                Some(session) => Some(session.entry.name.clone()),
                None => {
                    self.notifications
                        .push(Severity::Warning, "Connect to a remote server first");
                    return;
                }
            },
        };

        let path = self.active_panel_path();
        self.bookmarks
            .add(config::bookmarks::Bookmark { label, path, host });
        self.save_bookmarks();
    }

    fn open_bookmarks_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let items: Vec<String> = self
            .bookmarks
            .iter()
            .map(|bookmark| match &bookmark.host {
                Some(host) => format!(
                    "{} \u{2014} {} [{host}]",
                    bookmark.label,
                    bookmark.path.display()
                ),
                None => format!("{} \u{2014} {}", bookmark.label, bookmark.path.display()),
            })
            .collect();

        self.dialog = Some(Dialog::List(
            ListDialog::new("Bookmarks", items).removable(true),
        ));
    }

    /// Navigates to a bookmark. A remote bookmark whose host has no active
    /// session warns instead of navigating — the list itself doesn't grey
    /// such entries out (`ListDialog` stays a plain string list), so this
    /// check is the only guard.
    fn navigate_to_bookmark(&mut self, index: usize) {
        let Some(bookmark) = self.bookmarks.get(index).cloned() else {
            return;
        };

        match bookmark.host {
            None => {
                self.active_panel = ActivePanel::Local;
                let result = self.local.navigate_to(bookmark.path);
                self.set_status(result);
            }
            Some(host) => {
                let Some(session_id) = self.sessions.by_host(&host).map(|session| session.id)
                else {
                    self.notifications
                        .push(Severity::Warning, format!("Connect to {host} first"));
                    return;
                };
                self.sessions.activate(session_id);
                self.active_panel = ActivePanel::Remote;
                self.spawn_remote_list(bookmark.path);
            }
        }
    }

    /// Saves `self.bookmarks` to disk, or does nothing if there's no real
    /// path to save to (`App::at`'s test construction) — mutations still
    /// apply to the in-memory list either way.
    fn save_bookmarks(&mut self) {
        if let Some(path) = self.bookmarks_path.clone() {
            let result = config::bookmarks::save_to(&path, &self.bookmarks);
            self.set_status(result);
        }
    }

    fn open_mkdir_dialog(&mut self) {
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

    fn open_rename_dialog(&mut self) {
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

    fn open_delete_dialog(&mut self) {
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
        }
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

/// Lists `path` over SFTP and reports the outcome — the tail end of every
/// remote panel operation (navigate, mkdir, rename, delete all finish by
/// refreshing the listing, just like their local counterparts do).
async fn relist(
    sftp: &SftpSession,
    session_id: u64,
    path: PathBuf,
    tx: &mpsc::UnboundedSender<PanelEvent>,
) {
    let path_str = path_to_remote_string(&path);
    match filesystem::remote::list(sftp, &path_str).await {
        Ok(entries) => {
            let _ = tx.send(PanelEvent::Listed {
                session_id,
                path,
                entries,
            });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(PanelEvent::Failed {
                session_id,
                message: errors::user_message(format!("Unable to list {}", path.display()), &err),
            });
        }
    }
}

/// SFTP paths are always POSIX-style strings; since TermConnect targets
/// Linux only, a `PathBuf`'s own `Display` already produces exactly that.
fn path_to_remote_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn notification_style(severity: Severity) -> Style {
    match severity {
        Severity::Info => Style::default(),
        Severity::Warning => Style::default().fg(Color::Yellow),
        Severity::Error => Style::default().fg(Color::Red),
    }
}

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

/// Builds the key-hint line from the live bindings, so a remapped action
/// shows its new key instead of a hardcoded default.
fn build_hint_text(bindings: &input::KeyBindings) -> String {
    let entries = [
        (Action::Help, "Help"),
        (Action::OpenConnections, "Connections"),
        (Action::Quit, "Quit"),
    ];

    entries
        .into_iter()
        .filter_map(|(action, label)| {
            bindings
                .key_for(action)
                .map(|spec| format!("{} {label}", input::format_key_spec(spec)))
        })
        .collect::<Vec<_>>()
        .join("  ")
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

/// Runs a full connection attempt in the background: dial, verify the host
/// key, authenticate in the roadmap's priority order (agent, key file,
/// password), then open the SFTP subsystem — reporting progress back over
/// `tx` so the UI never blocks on network I/O. A password prompt is
/// requested via a one-shot round-trip embedded in
/// [`ConnectEvent::NeedsPassword`].
async fn run_connect(entry: ConnectionEntry, tx: mpsc::UnboundedSender<ConnectEvent>) {
    let mut handle = match connection::client::connect(&entry.host, entry.port).await {
        Ok(handle) => handle,
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(format!("Unable to connect to {}", entry.name), &err),
            });
            return;
        }
    };

    match connection::client::authenticate_non_interactive(&mut handle, &entry).await {
        Ok(true) => {
            finish_connect(entry, handle, &tx).await;
            return;
        }
        Ok(false) => {}
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(
                    format!("Authentication error for {}", entry.name),
                    &err,
                ),
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

    match connection::client::authenticate_password(&mut handle, &entry.username, &password).await {
        Ok(true) => finish_connect(entry, handle, &tx).await,
        Ok(false) => {
            let _ = tx.send(ConnectEvent::Failed {
                message: format!("Authentication failed for {}", entry.name),
            });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(
                    format!("Authentication error for {}", entry.name),
                    &err,
                ),
            });
        }
    }
}

/// Opens the SFTP subsystem on a freshly-authenticated session and reports
/// the finished connection, or a failure if SFTP itself couldn't start.
async fn finish_connect(
    entry: ConnectionEntry,
    handle: russh::client::Handle<TermConnectHandler>,
    tx: &mpsc::UnboundedSender<ConnectEvent>,
) {
    match connection::client::open_sftp(&handle).await {
        Ok(sftp) => {
            let _ = tx.send(ConnectEvent::Connected {
                entry,
                handle,
                sftp,
            });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(
                    format!("Connected to {} but failed to start SFTP", entry.name),
                    &err,
                ),
            });
        }
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

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
}
