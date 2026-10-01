use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use porthmos_core::{
    Command, CoreHandle, Event, Location, Question, RequestId, Severity, ShellInvocation,
    config::{bookmarks::Bookmark, settings::PanelSettings},
    edit::{EditChoice, EditQuestionKind},
    profiles::{ConnectionEntry, ConnectionSource},
    transfer::{
        Direction, TransferSnapshot,
        conflicts::{ConflictInfo, Resolution},
        rows::{QueueRow, RowKind},
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    input::{self, Action},
    sessions::Sessions,
    terminal::{self, Backend},
    widgets::{
        connections_list,
        connections_view::{ConnectionsView, Selection},
        dialog::{
            ConfirmDialog, ConflictDialog, Dialog, DialogOutcome, ListDialog, TextInputDialog, message::MessageDialog,
        },
        help,
        history_view::{self, HistoryView},
        layout,
        notifications::Notifications,
        panel_view::{self, ActivePanel, PanelView},
        search_view::{self, SearchOutcome, SearchView},
        transfer_list,
    },
};

mod actions;
mod bookmarks;
mod conflicts;
mod connections;
mod dialogs;
mod edit;
mod filter;
mod history;
mod render;
mod search;
mod transfer_queue;
mod transfers;

const SHUTDOWN_WAIT: Duration = Duration::from_secs(35);

enum PendingAction {
    Mkdir,
    Rename,
    Delete,
    AddBookmark,
    SubmitPassword { request_id: RequestId },
    TrustHostKey { request_id: RequestId },
    TrustCertificate { request_id: RequestId },
    AddConnection,
    EditConnection { original: ConnectionEntry },
    DeleteConnection { name: String },
    EditSshLabels { name: String },
    FixMissingHost { name: String },
    MoveLabels { from: String, candidates: Vec<String> },
    ResolveConflict,
    ClearHistory,
    EditUpload { edit_id: u64 },
    EditConflict { edit_id: u64 },
    QuitWhileSaving,
}

enum Reveal {
    AwaitingSave(String),
    Ready(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Files,
    Connections,
    Search,
    Transfers,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConnectionStatus {
    Disconnected,
    Connecting(String),
    Failed(String),
}

struct SearchSession {
    view: SearchView,
    location: Location,
}

struct ConflictPrompt {
    batch_id: u64,
    files: Vec<ConflictInfo>,
    answers: Vec<Option<Resolution>>,
}

pub struct App {
    should_quit: bool,
    screen: Screen,
    active_panel: ActivePanel,
    local: PanelView,
    sessions: Sessions,
    dialog: Option<Dialog>,
    help_visible: bool,
    pending_action: Option<PendingAction>,
    notifications: Notifications,
    connections: ConnectionsView,
    reveal: Option<Reveal>,
    announced_missing: HashSet<String>,
    keyring_available: bool,
    keyring_waiting: bool,
    save_choice: bool,
    transfers_cursor: usize,
    reselect_row: Option<RowKind>,
    connection_status: ConnectionStatus,
    search: Option<SearchSession>,
    transfers: TransferSnapshot,
    history: HistoryView,
    edit_questions: VecDeque<edit::EditPrompt>,
    open_edit_question: Option<edit::EditPrompt>,
    edits_busy: bool,
    conflict_prompts: VecDeque<ConflictPrompt>,
    key_bindings: input::KeyBindings,
    bookmarks: Vec<Bookmark>,
    core: CoreHandle,
}

impl App {
    pub fn new(core: CoreHandle, local_path: PathBuf, panel: &PanelSettings, key_bindings: input::KeyBindings) -> Self {
        let app = Self {
            should_quit: false,
            screen: Screen::Files,
            active_panel: ActivePanel::Local,
            local: PanelView::new(local_path.clone(), panel.sort, panel.show_hidden),
            sessions: Sessions::new(),
            dialog: None,
            help_visible: false,
            pending_action: None,
            notifications: Notifications::default(),
            connections: ConnectionsView::new(),
            reveal: None,
            announced_missing: HashSet::new(),
            keyring_available: true,
            keyring_waiting: false,
            save_choice: false,
            transfers_cursor: 0,
            reselect_row: None,
            connection_status: ConnectionStatus::Disconnected,
            search: None,
            transfers: TransferSnapshot::default(),
            history: HistoryView::new(),
            edit_questions: VecDeque::new(),
            open_edit_question: None,
            edits_busy: false,
            conflict_prompts: VecDeque::new(),
            key_bindings,
            bookmarks: Vec::new(),
            core,
        };
        app.core.send(Command::List { location: Location::Local, path: Some(local_path) });
        app
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.notifications.push(Severity::Warning, message);
    }

    pub async fn run(
        &mut self, terminal: &mut ratatui::Terminal<Backend>, mut core_events: UnboundedReceiver<Event>,
    ) -> Result<()> {
        let mut events = EventStream::new();

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                event = events.next() => {
                    if let Some(event) = event
                        && let TerminalEvent::Key(key) = event?
                        && key.kind == KeyEventKind::Press
                    {
                        self.apply_key(key);
                    }
                }
                Some(core_event) = core_events.recv() => {
                    if let Event::ShellReady { invocation, .. } = core_event {
                        self.launch_shell(terminal, invocation).await?;
                        events = EventStream::new();
                    } else if let Event::EditReady { edit_id, file, editor } = core_event {
                        self.launch_editor(terminal, edit_id, file, editor).await?;
                        events = EventStream::new();
                    } else {
                        self.apply_core_event(core_event);
                    }
                }
                _ = sleep_until_or_pending(self.notifications.next_wake()) => {
                    self.notifications.expire(Instant::now());
                }
            }
        }

        let _ = tokio::time::timeout(SHUTDOWN_WAIT, self.core.shutdown()).await;
        Ok(())
    }

    fn apply_key(&mut self, key: KeyEvent) {
        if self.help_visible {
            self.help_visible = false;
        } else if self.dialog.is_some() {
            self.apply_dialog_key(key);
            self.after_dialog_key();
        } else if self.screen == Screen::Search {
            self.apply_search_key(key);
        } else if self.editing_filter() {
            self.apply_filter_key(key);
        } else {
            let action = self.key_bindings.map_key(key);
            if action == Action::Noop && self.screen == Screen::Connections && key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Right => self.connections.expand(),
                    KeyCode::Left => self.connections.collapse_or_parent(),
                    _ => {}
                }
            } else {
                self.apply_action(action);
            }
        }
    }

    fn apply_core_event(&mut self, event: Event) {
        match event {
            Event::Notice { severity, message } => self.notifications.push(severity, message),
            Event::Connecting { name } => self.connection_status = ConnectionStatus::Connecting(name),
            Event::Connected { session, name, shell_available } => self.apply_connected(session, name, shell_available),
            Event::ConnectFailed { message, .. } => {
                self.connection_status = ConnectionStatus::Failed(message.clone());
                self.notifications.push(Severity::Error, message);
            }
            Event::Question { request_id, question } => self.ask(request_id, question),
            Event::Disconnected { session, .. } => {
                self.sessions.remove(session);
            }
            Event::ShellReady { .. } => {}
            Event::Listed { location, path, entries } => self.apply_listing(location, path, entries),
            Event::LocationChanged { location } => self.relist(location),
            Event::Profiles(entries) => {
                self.connections.replace(entries);
                match self.reveal.take() {
                    Some(Reveal::Ready(name)) => self.connections.reveal(&name),
                    other => self.reveal = other,
                }
                self.announce_missing_hosts();
            }
            Event::ProfileSaved => {
                self.dialog = None;
                self.pending_action = None;
                if let Some(Reveal::AwaitingSave(name)) = self.reveal.take() {
                    self.reveal = Some(Reveal::Ready(name));
                }
            }
            Event::ProfileRejected { message } => self.set_form_error(message),
            Event::Bookmarks(bookmarks) => self.bookmarks = bookmarks,
            Event::TransfersChanged(snapshot) => self.apply_transfer_snapshot(snapshot),
            Event::ConflictsFound { batch_id, files } => self.queue_conflict_prompt(batch_id, files),
            Event::ConflictsWithdrawn { batch_ids } => self.drop_conflict_prompts(&batch_ids),
            Event::SearchFound(entry) => {
                if let Some(search) = self.search.as_mut() {
                    search.view.push_result(entry);
                }
            }
            Event::SearchDone { truncated } => {
                if let Some(search) = self.search.as_mut() {
                    search.view.finish(truncated);
                }
            }
            Event::SearchFailed(message) => {
                if let Some(search) = self.search.as_mut() {
                    search.view.finish(false);
                    self.notifications.push(Severity::Error, message);
                }
            }
            Event::KeyringStatus { available } => self.keyring_available = available,
            Event::KeyringWaiting { waiting } => self.keyring_waiting = waiting,
            Event::SaveChoice { save } => self.save_choice = save,
            Event::History(entries) => self.history.replace(entries),
            Event::EditReady { .. } => {}
            Event::EditsBusy(busy) => self.edits_busy = busy,
            Event::EditQuestion { edit_id, name, kind } => self.ask_edit_question(edit_id, &name, kind),
        }
    }

    fn ask(&mut self, request_id: RequestId, question: Question) {
        self.requeue_open_edit_question();
        match question {
            Question::Password { username, name } => self.open_password_prompt(request_id, username, name),
            Question::TrustHostKey { name, host, port, key_type, fingerprint } => {
                let message = format!(
                    "{name} ({host}:{port}) is not a known host.\n\
                     {key_type} {fingerprint}\n\
                     Trust this key and add it to ~/.ssh/known_hosts?"
                );
                self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(message)));
                self.pending_action = Some(PendingAction::TrustHostKey { request_id });
            }
            Question::TrustCertificate { name, host, port, fingerprint, subject, expires } => {
                let message = format!(
                    "{name} ({host}:{port}) presented a certificate that isn't trusted.\n\
                     Subject: {subject}   Expires: {expires}\n\
                     {fingerprint}\n\
                     Trust it and remember it?"
                );
                self.dialog = Some(Dialog::Confirm(ConfirmDialog::new(message)));
                self.pending_action = Some(PendingAction::TrustCertificate { request_id });
            }
        }
    }

    fn apply_listing(&mut self, location: Location, path: PathBuf, entries: Vec<porthmos_core::Entry>) {
        match location {
            Location::Local => {
                if path != self.local.path() {
                    self.local.cursor = 0;
                }
                self.local.replace_listing(path, entries);
            }
            Location::Session(id) => {
                if let Some(session) = self.sessions.by_id_mut(id) {
                    session.panel.replace_listing(path, entries);
                } else {
                    tracing::debug!("dropping a listing for session {id}, which is gone");
                }
            }
        }
    }

    fn relist(&self, location: Location) {
        let path = match location {
            Location::Local => Some(self.local.path().to_path_buf()),
            Location::Session(id) => match self.sessions.by_id(id) {
                Some(session) => Some(session.panel.path().to_path_buf()),
                None => return,
            },
        };
        self.core.send(Command::List { location, path });
    }

    fn active_location(&self) -> Option<Location> {
        match self.active_panel {
            ActivePanel::Local => Some(Location::Local),
            ActivePanel::Remote => self.sessions.active_id().map(Location::Session),
        }
    }

    fn panel(&self, location: Location) -> Option<&PanelView> {
        match location {
            Location::Local => Some(&self.local),
            Location::Session(id) => self.sessions.by_id(id).map(|session| &session.panel),
        }
    }
}

async fn sleep_until_or_pending(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
