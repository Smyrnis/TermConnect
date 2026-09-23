use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};
use russh_sftp::client::SftpSession;
use tokio::sync::{mpsc, oneshot};

use crate::{
    config,
    connection::{self, ConnectionEntry, ConnectionSource, client::TermConnectHandler},
    errors,
    filesystem::{self, Entry, path_to_remote_string, search::SearchEvent},
    terminal,
    transfer::{self, Direction, JobStatus, TransferOutcome, TransferQueue},
    tui::{
        self, Backend, connections_list,
        dialog::{ConfirmDialog, Dialog, DialogOutcome, ListDialog, TextInputDialog},
        help,
        input::{self, Action},
        layout,
        notifications::{Notifications, Severity},
        panels::{self, ActivePanel, PanelState},
        search_view,
        search_view::{SearchOutcome, SearchView},
        sort::{SortKey, SortOrder},
        transfer_list,
    },
};

mod actions;
mod bookmarks;
mod connections;
mod dialogs;
mod render;
mod search;
mod transfer_queue;
mod transfers;

enum PendingAction {
    Mkdir,
    Rename,
    Delete,
    AddBookmark,
    SubmitPassword,
    AddConnection,
    EditConnection { original: ConnectionEntry },
    DeleteConnection { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Files,
    Connections,
    Search,
    Transfers,
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
    generation: Arc<AtomicU64>,
}

enum ConnectEvent {
    Connected { entry: Box<ConnectionEntry>, handle: russh::client::Handle<TermConnectHandler>, sftp: SftpSession },
    NeedsPassword { name: String, username: String, respond_to: oneshot::Sender<String> },
    Failed { message: String },
}

enum PanelEvent {
    Listed { session_id: u64, path: PathBuf, entries: Vec<Entry> },
    Failed { session_id: u64, message: String },
}

enum TransferEvent {
    Progress { id: u64, transferred: u64 },
    Finished { id: u64, outcome: TransferOutcome },
    Failed { id: u64, message: String },
    PlanReady { batch_id: u64, session_id: u64, direction: Direction, plan: transfer::plan::DirectoryPlan },
    PlanFailed { batch_id: u64, message: String },
    PlanCancelled { batch_id: u64, session_id: u64, direction: Direction },
}

struct PlanningScan {
    batch_id: u64,
    session_id: u64,
    direction: Direction,
    display_name: String,
    cancel: Arc<AtomicBool>,
}

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
    transfers_cursor: usize,
    connection_status: ConnectionStatus,
    search: Option<SearchSession>,
    search_tx: mpsc::UnboundedSender<SearchEvent>,
    search_rx: mpsc::UnboundedReceiver<SearchEvent>,
    connect_tx: mpsc::UnboundedSender<ConnectEvent>,
    connect_rx: mpsc::UnboundedReceiver<ConnectEvent>,
    panel_tx: mpsc::UnboundedSender<PanelEvent>,
    panel_rx: mpsc::UnboundedReceiver<PanelEvent>,
    transfers: TransferQueue,
    max_parallel: usize,
    transfer_cancels: HashMap<u64, Arc<AtomicBool>>,
    transfer_tx: mpsc::UnboundedSender<TransferEvent>,
    transfer_rx: mpsc::UnboundedReceiver<TransferEvent>,
    planning: Vec<PlanningScan>,
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

        let mut app = Self::at_with(std::env::current_dir()?, &settings.panel, key_bindings, bookmarks, Some(bookmarks_path))?;
        app.max_parallel = settings.transfers.max_parallel;

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

    #[cfg(test)]
    fn at(path: PathBuf) -> Result<Self> {
        Self::at_with(path, &config::settings::PanelSettings::default(), input::KeyBindings::defaults(), config::bookmarks::Bookmarks::default(), None)
    }

    fn at_with(path: PathBuf, panel_settings: &config::settings::PanelSettings, key_bindings: input::KeyBindings, bookmarks: config::bookmarks::Bookmarks, bookmarks_path: Option<PathBuf>) -> Result<Self> {
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
            transfers_cursor: 0,
            connection_status: ConnectionStatus::Disconnected,
            search: None,
            search_tx,
            search_rx,
            connect_tx,
            connect_rx,
            panel_tx,
            panel_rx,
            transfers: TransferQueue::new(),
            max_parallel: config::settings::TransferSettings::default().max_parallel,
            transfer_cancels: HashMap::new(),
            transfer_tx,
            transfer_rx,
            planning: Vec::new(),
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
                    self.drain_pending_transfer_events();
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

    fn set_status(&mut self, result: Result<()>) {
        if let Err(err) = result {
            self.notifications.push(Severity::Error, err.to_string());
        }
    }
}

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

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

async fn sleep_until_or_pending(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
#[path = "../../tests/app/mod_test.rs"]
mod tests;
