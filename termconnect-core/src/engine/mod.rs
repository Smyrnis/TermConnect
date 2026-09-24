mod command;
mod event;
mod handlers;
mod prompter;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
    time::{Duration, Instant},
};

pub use command::{Command, Location, RequestId, SessionId};
pub use event::Event;
use prompter::PendingQuestions;
use termconnect_vfs::{Environment, FileSystem, Protocol};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    Paths, Severity,
    config::{bookmarks::Bookmarks, settings::TransferSettings},
    profiles::ConnectionEntry,
    transfer::{
        Direction, TransferOutcome, TransferQueue, TransferSnapshot,
        conflicts::ConflictPolicy,
        plan::DirectoryPlan,
        rows::{RowState, ScanInfo},
    },
};

const PROGRESS_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) enum TransferEvent {
    Progress { id: u64, transferred: u64 },
    Finished { id: u64, outcome: TransferOutcome },
    Failed { id: u64, message: String },
    PlanReady { batch_id: u64, session_id: u64, direction: Direction, plan: DirectoryPlan },
    PlanFailed { batch_id: u64, message: String },
    PlanCancelled { batch_id: u64, session_id: u64, direction: Direction },
    PartialsRemoved { session_id: u64 },
}

pub(crate) enum Internal {
    Connected { entry: ConnectionEntry, protocol: Arc<dyn Protocol>, fs: Arc<dyn FileSystem> },
    ConnectFailed { name: String, message: String },
    Transfer(TransferEvent),
}

pub(crate) struct PlanningScan {
    batch_id: u64,
    session_id: u64,
    direction: Direction,
    display_name: String,
    cancel: Arc<AtomicBool>,
}

pub(crate) struct ConflictReview {
    batch_id: u64,
    session_id: u64,
    direction: Direction,
    plan: DirectoryPlan,
}

pub(crate) struct LiveSession {
    name: String,
    entry: ConnectionEntry,
    protocol: Arc<dyn Protocol>,
    fs: Arc<dyn FileSystem>,
}

struct SearchState {
    cancel: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

pub(crate) struct Engine {
    paths: Paths,
    env: Environment,
    protocols: Vec<Arc<dyn Protocol>>,
    local_fs: Arc<dyn FileSystem>,
    events: UnboundedSender<Event>,
    internal: UnboundedSender<Internal>,
    sessions: BTreeMap<SessionId, LiveSession>,
    next_session_id: SessionId,
    questions: PendingQuestions,
    transfers: TransferQueue,
    max_parallel: usize,
    on_conflict: ConflictPolicy,
    transfer_cancels: HashMap<u64, Arc<AtomicBool>>,
    planning: Vec<PlanningScan>,
    reviews: VecDeque<ConflictReview>,
    search: SearchState,
    bookmarks: Bookmarks,
    published: TransferSnapshot,
    last_progress_publish: Option<Instant>,
}

pub(crate) struct EngineParts {
    pub(crate) paths: Paths,
    pub(crate) env: Environment,
    pub(crate) protocols: Vec<Arc<dyn Protocol>>,
    pub(crate) local_fs: Arc<dyn FileSystem>,
    pub(crate) transfers: TransferSettings,
    pub(crate) bookmarks: Bookmarks,
}

impl Engine {
    pub(crate) fn new(parts: EngineParts, events: UnboundedSender<Event>, internal: UnboundedSender<Internal>) -> Self {
        Self {
            paths: parts.paths,
            env: parts.env,
            protocols: parts.protocols,
            local_fs: parts.local_fs,
            events,
            internal,
            sessions: BTreeMap::new(),
            next_session_id: 0,
            questions: PendingQuestions::default(),
            transfers: TransferQueue::new(),
            max_parallel: parts.transfers.max_parallel,
            on_conflict: parts.transfers.on_conflict,
            transfer_cancels: HashMap::new(),
            planning: Vec::new(),
            reviews: VecDeque::new(),
            search: SearchState { cancel: Arc::new(AtomicBool::new(false)), generation: Arc::new(AtomicU64::new(0)) },
            bookmarks: parts.bookmarks,
            published: TransferSnapshot::default(),
            last_progress_publish: None,
        }
    }

    pub(crate) async fn run(
        mut self, mut commands: UnboundedReceiver<Command>, mut internal: UnboundedReceiver<Internal>,
    ) {
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Shutdown) | None => break,
                    Some(command) => self.handle_command(command),
                },
                Some(done) = internal.recv() => self.handle_internal(done),
            }
        }
    }

    pub(crate) fn handle_command(&mut self, command: Command) {
        match command {
            Command::Connect { profile } => self.connect(&profile),
            Command::Disconnect { session } => self.disconnect(session),
            Command::Answer { request_id, answer } => self.questions.answer(request_id, answer),
            Command::PrepareShell { session } => self.prepare_shell(session),
            Command::List { location, path } => self.list(location, path),
            Command::CreateDir { location, path } => self.create_dir(location, path),
            Command::Rename { location, from, to } => self.rename(location, from, to),
            Command::Delete { location, paths } => self.delete(location, paths),
            Command::Copy { from, entries, to, dest_dir } => self.copy(from, entries, to, dest_dir),
            Command::ResolveConflicts { batch_id, answers } => self.resolve_conflicts(batch_id, answers),
            Command::CancelAllTransfers => self.cancel_all_copies(),
            Command::CancelRow { kind } => self.cancel_row(kind),
            Command::RetryRow { kind } => self.retry_row(kind),
            Command::ClearFinished => self.clear_finished_rows(),
            Command::Search { location, root, pattern } => self.search(location, root, pattern),
            Command::CancelSearch => self.cancel_search(),
            Command::ListProfiles => self.list_profiles(),
            Command::SaveProfile { original, draft } => self.save_profile(original, draft),
            Command::DeleteProfile { name } => self.delete_profile(&name),
            Command::AddBookmark { label, location, path } => self.add_bookmark(label, location, path),
            Command::RemoveBookmark { index } => self.remove_bookmark(index),
            Command::Shutdown => {}
        }
        self.publish_transfers();
    }

    pub(crate) fn handle_internal(&mut self, done: Internal) {
        match done {
            Internal::Connected { entry, protocol, fs } => self.finish_connect(entry, protocol, fs),
            Internal::ConnectFailed { name, message } => self.emit(Event::ConnectFailed { name, message }),
            Internal::Transfer(TransferEvent::Progress { id, transferred }) => {
                if let Some(job) = self.transfers.get_mut(id) {
                    job.transferred_bytes = transferred;
                }
                let due = self
                    .last_progress_publish
                    .is_none_or(|published_at| published_at.elapsed() >= PROGRESS_SNAPSHOT_INTERVAL);
                if due {
                    self.publish_transfers();
                }
                return;
            }
            Internal::Transfer(event) => self.handle_transfer_event(event),
        }
        self.publish_transfers();
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn notice(&self, severity: Severity, message: impl Into<String>) {
        self.emit(Event::Notice { severity, message: message.into() });
    }

    fn fs_for(&self, location: Location) -> Option<Arc<dyn FileSystem>> {
        match location {
            Location::Local => Some(self.local_fs.clone()),
            Location::Session(id) => self.sessions.get(&id).map(|session| session.fs.clone()),
        }
    }

    fn scans(&self) -> Vec<ScanInfo<'_>> {
        self.planning
            .iter()
            .map(|scan| ScanInfo {
                batch_id: scan.batch_id,
                label: &scan.display_name,
                direction: scan.direction,
                state: RowState::Scanning,
            })
            .chain(self.reviews.iter().map(|review| ScanInfo {
                batch_id: review.batch_id,
                label: self.transfers.batch_label(review.batch_id).unwrap_or("copy"),
                direction: review.direction,
                state: RowState::AwaitingAnswer,
            }))
            .collect()
    }

    fn snapshot(&self) -> TransferSnapshot {
        TransferSnapshot::of(&self.transfers, &self.scans())
    }

    fn publish_transfers(&mut self) {
        let snapshot = self.snapshot();
        if snapshot != self.published {
            self.published = snapshot.clone();
            self.last_progress_publish = Some(Instant::now());
            self.emit(Event::TransfersChanged(snapshot));
        }
    }
}

#[cfg(test)]
pub(crate) mod testing;
