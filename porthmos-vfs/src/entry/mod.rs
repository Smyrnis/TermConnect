use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    pub size: u64,
    pub modified: Option<u64>,
    pub kind: FileKind,
    pub permissions: Option<u32>,
}

impl Metadata {
    pub fn is_dir(&self) -> bool {
        self.kind == FileKind::Dir
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirItem {
    pub name: String,
    pub path: PathBuf,
    pub metadata: Metadata,
}

pub fn path_to_remote_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn join_remote(parent: &str, name: &str) -> String {
    if parent.ends_with('/') { format!("{parent}{name}") } else { format!("{parent}/{name}") }
}

#[cfg(test)]
mod tests;
