pub mod local;
pub mod remote;
pub mod search;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: Option<u32>,
}

pub fn path_to_remote_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}
