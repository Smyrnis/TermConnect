use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use russh_sftp::client::SftpSession;
use tokio::sync::{mpsc, oneshot};

use crate::connection::client::TermConnectHandler;
use crate::connection::{self, ConnectionEntry};
use crate::filesystem::{self, Entry};
use crate::terminal;
use crate::transfer::{self, Direction, JobStatus, TransferOutcome, TransferQueue};
use crate::tui::input::{self, Action};
use crate::tui::panels::{self, ActivePanel, PanelState};
use crate::tui::widgets::connections_list;
use crate::tui::widgets::dialog::{ConfirmDialog, Dialog, DialogOutcome, TextInputDialog};
use crate::tui::{self, Backend, layout};

const HINT_TEXT: &str = "\u{2191}\u{2193} Navigate  Enter Open  Tab Switch  Space Select  F2 Rename  F4 Terminal  F5 Copy  F7 Mkdir  F8 Delete  Ctrl+R Refresh  Ctrl+C Cancel  F9 Connections  F10 Quit";

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
    Listed { path: PathBuf, entries: Vec<Entry> },
    Failed(String),
}

/// Progress reported by a running file transfer (see `run_transfer`).
enum TransferEvent {
    Progress { id: u64, transferred: u64 },
    Finished { id: u64, outcome: TransferOutcome },
    Failed { id: u64, message: String },
}

pub struct App {
    should_quit: bool,
    screen: Screen,
    active_panel: ActivePanel,
    local: PanelState,
    remote: Option<PanelState>,
    dialog: Option<Dialog>,
    pending_action: Option<PendingAction>,
    pending_password: Option<oneshot::Sender<String>>,
    status: Option<String>,
    connections: Vec<ConnectionEntry>,
    connections_cursor: usize,
    connection_status: ConnectionStatus,
    connection_handle: Option<russh::client::Handle<TermConnectHandler>>,
    active_connection: Option<ConnectionEntry>,
    sftp: Option<Arc<SftpSession>>,
    connect_tx: mpsc::UnboundedSender<ConnectEvent>,
    connect_rx: mpsc::UnboundedReceiver<ConnectEvent>,
    panel_tx: mpsc::UnboundedSender<PanelEvent>,
    panel_rx: mpsc::UnboundedReceiver<PanelEvent>,
    transfers: TransferQueue,
    active_transfer_cancel: Option<Arc<AtomicBool>>,
    transfer_tx: mpsc::UnboundedSender<TransferEvent>,
    transfer_rx: mpsc::UnboundedReceiver<TransferEvent>,
}

impl App {
    pub fn new() -> Result<Self> {
        Self::at(std::env::current_dir()?)
    }

    fn at(path: PathBuf) -> Result<Self> {
        let (connect_tx, connect_rx) = mpsc::unbounded_channel();
        let (panel_tx, panel_rx) = mpsc::unbounded_channel();
        let (transfer_tx, transfer_rx) = mpsc::unbounded_channel();

        Ok(Self {
            should_quit: false,
            screen: Screen::Files,
            active_panel: ActivePanel::Local,
            local: PanelState::new(path)?,
            remote: None,
            dialog: None,
            pending_action: None,
            pending_password: None,
            status: None,
            connections: Vec::new(),
            connections_cursor: 0,
            connection_status: ConnectionStatus::Disconnected,
            connection_handle: None,
            active_connection: None,
            sftp: None,
            connect_tx,
            connect_rx,
            panel_tx,
            panel_rx,
            transfers: TransferQueue::new(),
            active_transfer_cancel: None,
            transfer_tx,
            transfer_rx,
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
                            let action = input::map_key(key);
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

        match &self.remote {
            Some(remote) => panels::render_panel(
                frame,
                remote_area,
                "REMOTE",
                self.active_panel == ActivePanel::Remote,
                remote,
            ),
            None => {
                let remote_title = match &self.connection_status {
                    ConnectionStatus::Connected(name) => format!("REMOTE {name}"),
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
            Some(message) => (message.clone(), Style::default().fg(Color::Red)),
            None => match self.transfers.active() {
                Some(job) => (self.transfer_status_text(job), Style::default()),
                None => (HINT_TEXT.to_string(), Style::default()),
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
            Action::Delete => self.open_delete_dialog(),
            Action::Copy => self.start_copy(),
            Action::CancelTransfer => self.cancel_active_transfer(),
            Action::OpenConnections => self.open_connections_screen(),
            Action::Back => self.screen = Screen::Files,
            Action::Up | Action::Down | Action::ToggleSelect | Action::Open | Action::Refresh => {
                self.apply_screen_action(action);
            }
            // Handled specially in `run`, which has the `&mut Terminal`
            // this needs to suspend/resume the TUI around `ssh`.
            Action::OpenTerminal => {}
            Action::Noop => {}
        }
    }

    fn apply_screen_action(&mut self, action: Action) {
        match self.screen {
            Screen::Files => self.apply_panel_action(action),
            Screen::Connections => self.apply_connections_action(action),
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
            _ => Ok(()),
        };

        self.set_status(result);
    }

    /// Remote navigation/selection is instant (pure state), but anything
    /// that needs a fresh listing (`Open`, `Refresh`) has to go over the
    /// network, so it's dispatched to a background task instead of run
    /// inline — see `spawn_remote_list`.
    fn apply_remote_panel_action(&mut self, action: Action) {
        let Some(remote) = self.remote.as_mut() else {
            return;
        };

        match action {
            Action::Up => remote.move_cursor(-1),
            Action::Down => remote.move_cursor(1),
            Action::ToggleSelect => remote.toggle_selection(),
            Action::Open => {
                if let Some(target) = remote.target_path_for_open() {
                    self.spawn_remote_list(target);
                }
            }
            Action::Refresh => {
                let path = remote.path().to_path_buf();
                self.spawn_remote_list(path);
            }
            _ => {}
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
        let Some(entry) = self.active_connection.clone() else {
            self.status = Some("Connect to a server first".to_string());
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
                self.status = Some(format!("ssh exited with status {status}"));
            }
            Ok(Ok(_)) => {}
            Ok(Err(io_err)) => {
                self.status = Some(format!("Failed to launch ssh: {io_err}"));
            }
            Err(join_err) => {
                self.status = Some(format!("ssh task failed: {join_err}"));
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
            self.sftp = None;
            self.remote = None;
            self.active_connection = None;
            self.connection_status = ConnectionStatus::Disconnected;
            return;
        }

        self.connection_status = ConnectionStatus::Connecting(entry.name.clone());
        let tx = self.connect_tx.clone();
        tokio::spawn(run_connect(entry, tx));
    }

    fn apply_connect_event(&mut self, event: ConnectEvent) {
        match event {
            ConnectEvent::Connected {
                entry,
                handle,
                sftp,
            } => {
                self.connection_handle = Some(handle);
                self.sftp = Some(Arc::new(sftp));
                self.connection_status = ConnectionStatus::Connected(entry.name.clone());
                self.active_connection = Some(entry);
                self.status = None;
                self.spawn_initial_remote_listing();
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

    fn apply_panel_event(&mut self, event: PanelEvent) {
        match event {
            PanelEvent::Listed { path, entries } => {
                match self.remote.as_mut() {
                    Some(remote) => remote.replace_listing(path, entries),
                    None => self.remote = Some(PanelState::from_listing(path, entries)),
                }
                self.status = None;
            }
            PanelEvent::Failed(message) => self.status = Some(message),
        }
    }

    fn spawn_initial_remote_listing(&mut self) {
        let Some(sftp) = self.sftp.clone() else {
            return;
        };
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            let home = match sftp.canonicalize(".").await {
                Ok(home) => home,
                Err(err) => {
                    let _ = tx.send(PanelEvent::Failed(err.to_string()));
                    return;
                }
            };
            relist(&sftp, PathBuf::from(home), &tx).await;
        });
    }

    fn spawn_remote_list(&mut self, path: PathBuf) {
        let Some(sftp) = self.sftp.clone() else {
            return;
        };
        let tx = self.panel_tx.clone();
        tokio::spawn(async move { relist(&sftp, path, &tx).await });
    }

    fn spawn_remote_mkdir(&mut self, name: String) {
        let (Some(remote), Some(sftp)) = (self.remote.as_ref(), self.sftp.clone()) else {
            return;
        };
        let dir_path = remote.path().to_path_buf();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            let target = path_to_remote_string(&dir_path.join(&name));
            if let Err(err) = filesystem::remote::create_directory(&sftp, &target).await {
                let _ = tx.send(PanelEvent::Failed(err.to_string()));
                return;
            }
            relist(&sftp, dir_path, &tx).await;
        });
    }

    fn spawn_remote_rename(&mut self, new_name: String) {
        let (Some(remote), Some(sftp)) = (self.remote.as_ref(), self.sftp.clone()) else {
            return;
        };
        let Some(current_name) = remote.current_entry_name() else {
            return;
        };
        let dir_path = remote.path().to_path_buf();
        let from = path_to_remote_string(&dir_path.join(current_name));
        let to = path_to_remote_string(&dir_path.join(&new_name));
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            if let Err(err) = filesystem::remote::rename(&sftp, &from, &to).await {
                let _ = tx.send(PanelEvent::Failed(err.to_string()));
                return;
            }
            relist(&sftp, dir_path, &tx).await;
        });
    }

    fn spawn_remote_delete(&mut self) {
        let (Some(remote), Some(sftp)) = (self.remote.as_ref(), self.sftp.clone()) else {
            return;
        };
        let targets = remote.targets();
        let dir_path = remote.path().to_path_buf();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            for target in targets {
                let target_str = path_to_remote_string(&target);
                if let Err(err) = filesystem::remote::delete(&sftp, &target_str).await {
                    let _ = tx.send(PanelEvent::Failed(err.to_string()));
                    return;
                }
            }
            relist(&sftp, dir_path, &tx).await;
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
        let Some(remote) = &self.remote else {
            self.status = Some("Connect to a remote server first".to_string());
            return;
        };
        let remote_dir = remote.path().to_path_buf();
        let entries = self.local.target_entries();
        self.enqueue_transfers(Direction::Upload, entries, remote_dir);
    }

    fn enqueue_downloads(&mut self) {
        let Some(remote) = &self.remote else {
            return;
        };
        let local_dir = self.local.path().to_path_buf();
        let entries = remote.target_entries();
        self.enqueue_transfers(Direction::Download, entries, local_dir);
    }

    fn enqueue_transfers(&mut self, direction: Direction, entries: Vec<Entry>, dest_dir: PathBuf) {
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

            self.transfers
                .enqueue(direction, local_path, remote_path, entry.name, entry.size);
        }

        if skipped_dirs > 0 {
            let plural = if skipped_dirs == 1 { "y" } else { "ies" };
            self.status = Some(format!(
                "Copying directories isn't supported yet \u{2014} skipped {skipped_dirs} director{plural}"
            ));
        }
    }

    fn maybe_start_next_transfer(&mut self) {
        let Some(id) = self.transfers.next_to_run() else {
            return;
        };
        let Some(sftp) = self.sftp.clone() else {
            return;
        };
        let Some(job) = self.transfers.get_mut(id) else {
            return;
        };

        job.status = JobStatus::InProgress;
        job.attempts += 1;
        let direction = job.direction;
        let local_path = job.local_path.clone();
        let remote_path = job.remote_path.clone();

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
                Err(err) => TransferEvent::Failed {
                    id,
                    message: err.to_string(),
                },
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
                    self.status = Some(format!("Transfer failed: {message}"));
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
                if let Some(remote) = &self.remote {
                    let path = remote.path().to_path_buf();
                    self.spawn_remote_list(path);
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

    fn open_mkdir_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }
        if self.active_panel == ActivePanel::Remote && self.remote.is_none() {
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
                .remote
                .as_ref()
                .and_then(PanelState::current_entry_name),
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
            ActivePanel::Remote => match &self.remote {
                Some(remote) => remote.targets(),
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
                    Some(PendingAction::Delete) | None => {}
                }
            }
        }
    }

    fn set_status(&mut self, result: Result<()>) {
        self.status = result.err().map(|err| err.to_string());
    }
}

/// Lists `path` over SFTP and reports the outcome — the tail end of every
/// remote panel operation (navigate, mkdir, rename, delete all finish by
/// refreshing the listing, just like their local counterparts do).
async fn relist(sftp: &SftpSession, path: PathBuf, tx: &mpsc::UnboundedSender<PanelEvent>) {
    let path_str = path_to_remote_string(&path);
    match filesystem::remote::list(sftp, &path_str).await {
        Ok(entries) => {
            let _ = tx.send(PanelEvent::Listed { path, entries });
        }
        Err(err) => {
            let _ = tx.send(PanelEvent::Failed(err.to_string()));
        }
    }
}

/// SFTP paths are always POSIX-style strings; since TermConnect targets
/// Linux only, a `PathBuf`'s own `Display` already produces exactly that.
fn path_to_remote_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
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
            let _ = tx.send(ConnectEvent::Failed {
                message: err.to_string(),
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

    match connection::client::authenticate_password(&mut handle, &entry.username, &password).await {
        Ok(true) => finish_connect(entry, handle, &tx).await,
        Ok(false) => {
            let _ = tx.send(ConnectEvent::Failed {
                message: format!("Authentication failed for {}", entry.name),
            });
        }
        Err(err) => {
            let _ = tx.send(ConnectEvent::Failed {
                message: err.to_string(),
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
            let _ = tx.send(ConnectEvent::Failed {
                message: format!("Connected but failed to start SFTP: {err}"),
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
        app.remote = Some(PanelState::from_listing(
            PathBuf::from("/home/user"),
            vec![Entry {
                name: "child".to_string(),
                path: PathBuf::from("/home/user/child"),
                is_dir: true,
                size: 0,
            }],
        ));
        app.active_panel = ActivePanel::Remote;

        app.apply_action(Action::Down);

        assert_eq!(app.remote.as_ref().unwrap().cursor, 1);
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

    #[test]
    fn panel_event_listed_creates_the_remote_panel_if_absent() {
        let (_dir, mut app) = app_in_temp_dir();
        assert!(app.remote.is_none());

        app.apply_panel_event(PanelEvent::Listed {
            path: PathBuf::from("/home/user"),
            entries: Vec::new(),
        });

        assert!(app.remote.is_some());
        assert_eq!(
            app.remote.as_ref().unwrap().path(),
            std::path::Path::new("/home/user")
        );
    }

    #[test]
    fn panel_event_failed_sets_status() {
        let (_dir, mut app) = app_in_temp_dir();
        app.apply_panel_event(PanelEvent::Failed("boom".to_string()));
        assert_eq!(app.status.as_deref(), Some("boom"));
    }
}
