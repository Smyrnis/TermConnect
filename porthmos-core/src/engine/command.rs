use std::path::PathBuf;

use porthmos_vfs::{Answer, Entry};

use crate::{
    profiles::ProfileDraft,
    transfer::{conflicts::Resolution, rows::RowKind},
};

pub type SessionId = u64;
pub type RequestId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Location {
    Local,
    Session(SessionId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Connect { profile: String },
    Disconnect { session: SessionId },
    Answer { request_id: RequestId, answer: Option<Answer> },
    PrepareShell { session: SessionId },
    List { location: Location, path: Option<PathBuf> },
    CreateDir { location: Location, path: PathBuf },
    Rename { location: Location, from: PathBuf, to: PathBuf },
    Delete { location: Location, paths: Vec<PathBuf> },
    Copy { from: Location, entries: Vec<Entry>, to: Location, dest_dir: PathBuf },
    ResolveConflicts { batch_id: u64, answers: Option<Vec<Resolution>> },
    CancelAllTransfers,
    CancelRow { kind: RowKind },
    RetryRow { kind: RowKind },
    ClearFinished,
    Search { location: Location, root: PathBuf, pattern: String },
    CancelSearch,
    ListProfiles,
    SaveProfile { original: Option<String>, draft: ProfileDraft },
    DeleteProfile { name: String },
    AddBookmark { label: String, location: Location, path: PathBuf },
    RemoveBookmark { index: usize },
    Shutdown,
}
