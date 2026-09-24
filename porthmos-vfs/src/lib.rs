mod entry;
mod error;
mod fs;
mod prompt;
mod protocol;
pub mod search;

pub use async_trait::async_trait;
pub use entry::{DirItem, Entry, FileKind, Metadata, join_remote, path_to_remote_string};
pub use error::{ErrorKind, ProtocolError};
pub use fs::{FileSystem, Reader, Writer};
pub use prompt::{Answer, Prompter, Question};
pub use protocol::{Environment, Protocol, ShellInvocation, Target};
pub use search::{
    MAX_SEARCH_DEPTH, MAX_SEARCH_RESULTS, SearchEvent, SearchQuery, SearchSender, glob_match, walk_search,
};
#[cfg(feature = "testing")]
pub mod testing;
