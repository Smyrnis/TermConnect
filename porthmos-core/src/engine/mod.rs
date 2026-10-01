mod command;
mod event;
mod handlers;
mod prompter;
mod report;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
    time::{Duration, Instant},
};

pub use command::{Command, Location, RequestId, SessionId};
pub use event::Event;
use porthmos_vfs::{Environment, FileSystem, Protocol};
use prompter::PendingQuestions;
pub(crate) use report::Reporter;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{
    Paths, Severity,
    config::{
        bookmarks::Bookmarks,
        settings::{EditSettings, TransferSettings},
    },
    history::History,
    profiles::ConnectionEntry,
    secrets::Secrets,
    tasks::{Scope, Tasks},
    transfer::{
        Direction, JobStatus, TransferOutcome, TransferQueue, TransferSnapshot,
        conflicts::ConflictPolicy,
        plan::DirectoryPlan,
        rows::{RowState, ScanInfo},
    },
};

pub(crate) const PROGRESS_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(100);
pub(crate) const PERSIST_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);
pub(crate) const PERSIST_FLUSH_GRACE: Duration = Duration::from_secs(5);
pub(crate) const EDIT_UPLOAD_GRACE: Duration = Duration::from_secs(30);

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
    Connected {
        entry: ConnectionEntry,
        protocol: Arc<dyn Protocol>,
        fs: Arc<dyn FileSystem>,
        typed: Option<prompter::TypedPassword>,
    },
    ConnectFailed {
        name: String,
        message: String,
    },
    Transfer(TransferEvent),
    Edit(handlers::EditEvent),
    KeyringProbed {
        available: bool,
    },
    KeyringDone(handlers::KeyringDone),
    PersistFailed {
        path: PathBuf,
        message: String,
        tag: u64,
    },
    TaskPanicked {
        name: &'static str,
        scope: Scope,
        message: String,
    },
}

pub(crate) struct PlanningScan {
    batch_id: u64,
    session_id: u64,
    direction: Direction,
    display_name: String,
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
    generation: Arc<AtomicU64>,
}

pub(crate) struct Engine {
    paths: Paths,
    env: Environment,
    protocols: Vec<Arc<dyn Protocol>>,
    local_fs: Arc<dyn FileSystem>,
    events: UnboundedSender<Event>,
    internal: UnboundedSender<Internal>,
    tasks: Tasks,
    reporter: Reporter,
    clock: fn() -> chrono::DateTime<chrono::Utc>,
    pub(crate) finished: Option<tokio::sync::watch::Sender<bool>>,
    pub(crate) shutdown_grace: Duration,
    pub(crate) upload_grace: Duration,
    pub(crate) flush_grace: Duration,
    state_protected: bool,
    writer: crate::persist::writer::Writer,
    sessions: BTreeMap<SessionId, LiveSession>,
    next_session_id: SessionId,
    questions: PendingQuestions,
    transfers: TransferQueue,
    max_parallel: usize,
    on_conflict: ConflictPolicy,
    planning: Vec<PlanningScan>,
    reviews: VecDeque<ConflictReview>,
    search: SearchState,
    bookmarks: Bookmarks,
    published: TransferSnapshot,
    last_progress_publish: Option<Instant>,
    publish_interval: Duration,
    transfers_dirty: bool,
    secrets: Secrets,
    history: handlers::HistoryLog,
    edit: handlers::EditState,
    keyring_jobs: Option<UnboundedSender<handlers::KeyringJob>>,
    next_keyring_job: u64,
    latest_keyring_job: HashMap<String, u64>,
    connecting: std::collections::HashSet<String>,
}

pub(crate) struct EngineParts {
    pub(crate) paths: Paths,
    pub(crate) env: Environment,
    pub(crate) protocols: Vec<Arc<dyn Protocol>>,
    pub(crate) local_fs: Arc<dyn FileSystem>,
    pub(crate) transfers: TransferSettings,
    pub(crate) bookmarks: Bookmarks,
    pub(crate) secrets: Secrets,
    pub(crate) history: History,
    pub(crate) edit: EditSettings,
    pub(crate) publish_interval: Duration,
    pub(crate) persist_interval: Duration,
}

impl Engine {
    pub(crate) fn new(parts: EngineParts, events: UnboundedSender<Event>, internal: UnboundedSender<Internal>) -> Self {
        let waiting = events.clone();
        parts.secrets.on_waiting(move |waiting_now| {
            let _ = waiting.send(Event::KeyringWaiting { waiting: waiting_now });
        });
        let panics = internal.clone();
        let tasks = Tasks::new(move |name, scope, message| {
            let _ = panics.send(Internal::TaskPanicked { name, scope, message });
        });
        let failures = internal.clone();
        let writer = crate::persist::writer::Writer::new(tasks.clone(), parts.persist_interval, move |failure| {
            let _ = failures.send(Internal::PersistFailed {
                path: failure.path,
                message: failure.message,
                tag: failure.tag,
            });
        });
        let reporter = Reporter::new(events.clone());
        Self {
            paths: parts.paths,
            env: parts.env,
            protocols: parts.protocols,
            local_fs: parts.local_fs,
            events,
            internal,
            tasks,
            reporter,
            clock: chrono::Utc::now,
            finished: None,
            shutdown_grace: SHUTDOWN_GRACE,
            upload_grace: EDIT_UPLOAD_GRACE,
            flush_grace: PERSIST_FLUSH_GRACE,
            state_protected: false,
            writer,
            sessions: BTreeMap::new(),
            next_session_id: 0,
            questions: PendingQuestions::default(),
            transfers: TransferQueue::new(),
            max_parallel: parts.transfers.max_parallel,
            on_conflict: parts.transfers.on_conflict,
            planning: Vec::new(),
            reviews: VecDeque::new(),
            search: SearchState { generation: Arc::new(AtomicU64::new(0)) },
            bookmarks: parts.bookmarks,
            published: TransferSnapshot::default(),
            last_progress_publish: None,
            publish_interval: parts.publish_interval,
            transfers_dirty: false,
            secrets: parts.secrets,
            history: handlers::HistoryLog::new(parts.history),
            edit: handlers::EditState::new(parts.edit),
            keyring_jobs: None,
            next_keyring_job: 0,
            latest_keyring_job: HashMap::new(),
            connecting: std::collections::HashSet::new(),
        }
    }

    pub(crate) async fn run(
        mut self, mut commands: UnboundedReceiver<Command>, mut internal: UnboundedReceiver<Internal>,
    ) {
        loop {
            let flush_at = self.flush_deadline();
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Shutdown) | None => break,
                    Some(command) => self.handle_command(command),
                },
                Some(done) = internal.recv() => self.handle_internal(done),
                () = sleep_until_or_pending(flush_at) => self.publish_transfers(),
            }
        }
        if self.transfers_dirty {
            self.publish_transfers();
        }
        self.shutdown_work(&mut internal).await;
    }

    fn apply_late_result(&mut self, done: Internal) {
        match done {
            Internal::KeyringDone(done) => self.finish_keyring_job(done),
            Internal::Edit(event @ handlers::EditEvent::Uploaded { .. }) => self.handle_edit_event(event),
            Internal::PersistFailed { path, message, tag } => self.persist_failed(path, message, tag),
            _ => {}
        }
    }

    async fn apply_results_until<F: std::future::Future>(
        &mut self, internal: &mut UnboundedReceiver<Internal>, future: F,
    ) {
        tokio::pin!(future);
        loop {
            tokio::select! {
                _ = &mut future => break,
                Some(done) = internal.recv() => self.apply_late_result(done),
            }
        }
    }

    async fn shutdown_work(&mut self, internal: &mut UnboundedReceiver<Internal>) {
        self.cancel_all_work();
        self.tasks.cancel_all();
        self.record_interrupted();
        let _ = tokio::time::timeout(self.flush_grace, self.writer.flush()).await;
        self.writer.close();
        self.keyring_jobs = None;
        let tasks = self.tasks.clone();
        if tasks.live_names().contains(&"edit-upload") {
            self.apply_results_until(internal, tasks.wait_for_name("edit-upload", self.upload_grace)).await;
        }
        self.apply_results_until(internal, tasks.shutdown(self.shutdown_grace)).await;
        while let Ok(done) = internal.try_recv() {
            self.apply_late_result(done);
        }
        if let Some(finished) = self.finished.take() {
            let _ = finished.send(true);
        }
    }

    pub(crate) fn handle_command(&mut self, command: Command) {
        match command {
            Command::Connect { profile } => self.connect(&profile),
            Command::Disconnect { session } => self.disconnect(session),
            Command::Answer { request_id, answer, save } => self.questions.answer(request_id, answer, save),
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
            Command::SaveProfile { original, draft } => self.save_profile(original, *draft),
            Command::DeleteProfile { name } => self.delete_profile(&name),
            Command::SaveSshLabels { name, group, tags } => self.save_ssh_labels(&name, &group, &tags),
            Command::MoveSshLabels { from, to } => self.move_ssh_labels(&from, &to),
            Command::ForgetSshLabels { name } => self.forget_ssh_labels(&name),
            Command::RememberSaveChoice { save } => self.remember_save_choice(save),
            Command::ForgetSshPassword { alias } => self.forget_ssh_password(&alias),
            Command::AddBookmark { label, location, path } => self.add_bookmark(label, location, path),
            Command::RemoveBookmark { index } => self.remove_bookmark(index),
            Command::ListHistory => self.publish_history(),
            Command::ClearHistory => self.clear_history(),
            Command::EditFile { location, path } => self.edit_file(location, path),
            Command::FinishEdit { edit_id, exit } => self.finish_edit(edit_id, exit),
            Command::ResolveEdit { edit_id, choice } => self.resolve_edit(edit_id, choice),
            Command::Shutdown => {}
        }
        self.process_row_changes();
        self.publish_edit_busy();
        self.request_publish();
    }

    fn recover_from_panic(&mut self, name: &'static str, scope: Scope, message: &str) {
        tracing::error!(target: "porthmos::tasks", task = name, "{message}");
        let failed = format!("A background task failed: {name}");
        match scope {
            Scope::Transfer(id) if self.transfers.get(id).is_some_and(|job| job.status == JobStatus::InProgress) => {
                self.handle_transfer_event(TransferEvent::Failed { id, message: failed });
            }
            Scope::Planning(batch_id) if self.planning.iter().any(|scan| scan.batch_id == batch_id) => {
                self.handle_transfer_event(TransferEvent::PlanFailed { batch_id, message: failed });
            }
            Scope::Edit(edit_id) if self.edit_in_progress(edit_id) => self.fail_edit(edit_id, &failed),
            _ => self.announce(Severity::Error, failed),
        }
    }

    pub(crate) fn handle_internal(&mut self, done: Internal) {
        match done {
            Internal::Connected { entry, protocol, fs, typed } => self.finish_connect(entry, protocol, fs, typed),
            Internal::ConnectFailed { name, message } => {
                self.connecting.remove(&name);
                self.connect_failed(name, message)
            }
            Internal::Edit(event) => self.handle_edit_event(event),
            Internal::KeyringProbed { available } => self.emit(Event::KeyringStatus { available }),
            Internal::KeyringDone(done) => self.finish_keyring_job(done),
            Internal::PersistFailed { path, message, tag } => self.persist_failed(path, message, tag),
            Internal::TaskPanicked { name, scope, message } => self.recover_from_panic(name, scope, &message),
            Internal::Transfer(TransferEvent::Progress { id, transferred }) => {
                if let Some(mut job) = self.transfers.get_mut(id) {
                    job.transferred_bytes = transferred;
                }
                self.request_publish();
                return;
            }
            Internal::Transfer(event) => self.handle_transfer_event(event),
        }
        self.process_row_changes();
        self.publish_edit_busy();
        self.request_publish();
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    #[track_caller]
    pub(crate) fn connect_failed(&self, name: String, message: String) {
        let severity = if message == crate::CONNECTION_CANCELLED { Severity::Info } else { Severity::Error };
        self.reporter.log(severity, &message);
        self.emit(Event::ConnectFailed { name, message });
    }

    #[track_caller]
    pub(crate) fn profile_rejected(&self, message: impl Into<String>) {
        let message = message.into();
        self.reporter.log(Severity::Warning, &message);
        self.emit(Event::ProfileRejected { message });
    }

    #[track_caller]
    pub(crate) fn report(&self, severity: Severity, message: impl Into<String>) {
        self.reporter.report(severity, message);
    }

    pub(crate) fn announce(&self, severity: Severity, message: impl Into<String>) {
        self.reporter.show(severity, message);
    }

    pub(crate) fn info(&self, message: impl Into<String>) {
        self.emit(Event::Notice { severity: Severity::Info, message: message.into() });
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
        self.process_row_changes();
        self.transfers_dirty = false;
        self.last_progress_publish = Some(Instant::now());
        let snapshot = self.snapshot();
        if snapshot != self.published {
            self.published = snapshot.clone();
            self.emit(Event::TransfersChanged(snapshot));
        }
    }

    fn request_publish(&mut self) {
        let due = self.publish_interval.is_zero()
            || self.last_progress_publish.is_none_or(|published_at| published_at.elapsed() >= self.publish_interval);
        if due {
            self.publish_transfers();
        } else {
            self.transfers_dirty = true;
        }
    }

    fn flush_deadline(&self) -> Option<Instant> {
        if !self.transfers_dirty {
            return None;
        }
        self.last_progress_publish.map(|published_at| published_at + self.publish_interval)
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
