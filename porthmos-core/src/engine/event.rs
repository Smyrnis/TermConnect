use std::path::PathBuf;

use porthmos_vfs::{Entry, Question, ShellInvocation};

use super::{Location, RequestId, SessionId};
use crate::{
    Severity,
    config::bookmarks::Bookmark,
    profiles::ConnectionEntry,
    transfer::{TransferSnapshot, conflicts::ConflictInfo},
};

#[derive(Debug, Clone)]
pub enum Event {
    Notice { severity: Severity, message: String },
    Connecting { name: String },
    Connected { session: SessionId, name: String, shell_available: bool },
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
}
