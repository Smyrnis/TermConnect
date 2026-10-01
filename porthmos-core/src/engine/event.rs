use std::path::PathBuf;

use porthmos_vfs::{Entry, Question, ShellInvocation};

use super::{Location, RequestId, SessionId};
use crate::{
    Severity,
    config::bookmarks::Bookmark,
    edit::{EditQuestionKind, EditorCommand},
    history::HistoryEntry,
    profiles::ConnectionEntry,
    transfer::{TransferSnapshot, conflicts::ConflictInfo},
};

#[derive(Debug, Clone)]
pub enum Event {
    Notice { severity: Severity, message: String },
    Connecting { name: String },
    Connected { session: SessionId, name: String, shell_available: bool },
    SessionCapabilities { session: SessionId, preserves_times: bool },
    ConnectFailed { name: String, message: String },
    Question { request_id: RequestId, question: Question },
    Disconnected { session: SessionId, name: String },
    ShellReady { session: SessionId, invocation: ShellInvocation },
    Listed { location: Location, path: PathBuf, entries: Vec<Entry> },
    LocationChanged { location: Location },
    Profiles(Vec<ConnectionEntry>),
    ProfileSaved,
    ProfileRejected { message: String },
    Bookmarks(Vec<Bookmark>),
    TransfersChanged(TransferSnapshot),
    ConflictsFound { batch_id: u64, files: Vec<ConflictInfo> },
    ConflictsWithdrawn { batch_ids: Vec<u64> },
    SearchFound(Entry),
    SearchDone { truncated: bool },
    SearchFailed(String),
    KeyringStatus { available: bool },
    KeyringWaiting { waiting: bool },
    SaveChoice { save: bool },
    History(Vec<HistoryEntry>),
    EditReady { edit_id: u64, file: PathBuf, editor: EditorCommand },
    EditQuestion { edit_id: u64, name: String, kind: EditQuestionKind },
    EditsBusy(bool),
}
