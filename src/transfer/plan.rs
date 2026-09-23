use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

pub struct PlannedFile {
    pub local_path: PathBuf,
    pub remote_path: String,
    pub display_name: String,
    pub size: u64,
}

pub struct DirectoryPlan {
    pub files: Vec<PlannedFile>,
    pub skipped_symlinks: usize,
}

pub enum PlanOutcome {
    Ready(DirectoryPlan),
    Cancelled,
}

enum Walk {
    Completed,
    Cancelled,
}

struct DiscoveredTree {
    directories: Vec<PathBuf>,
    files: Vec<(PathBuf, u64)>,
    skipped_symlinks: usize,
}

fn discover_local_tree(root: &Path, cancel: &AtomicBool) -> Result<Option<DiscoveredTree>> {
    let mut tree = DiscoveredTree { directories: Vec::new(), files: Vec::new(), skipped_symlinks: 0 };
    match discover_local_tree_into(root, Path::new(""), &mut tree, cancel)? {
        Walk::Completed => Ok(Some(tree)),
        Walk::Cancelled => Ok(None),
    }
}

fn discover_local_tree_into(
    root: &Path, relative: &Path, tree: &mut DiscoveredTree, cancel: &AtomicBool,
) -> Result<Walk> {
    for dir_entry in fs::read_dir(root.join(relative))? {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Walk::Cancelled);
        }
        let dir_entry = dir_entry?;
        let file_type = dir_entry.file_type()?;
        let entry_relative = relative.join(dir_entry.file_name());

        if file_type.is_symlink() {
            tree.skipped_symlinks += 1;
            continue;
        }

        if file_type.is_dir() {
            tree.directories.push(entry_relative.clone());
            if let Walk::Cancelled = discover_local_tree_into(root, &entry_relative, tree, cancel)? {
                return Ok(Walk::Cancelled);
            }
        } else {
            let size = dir_entry.metadata()?.len();
            tree.files.push((entry_relative, size));
        }
    }
    Ok(Walk::Completed)
}

fn ensure_local_directory(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    crate::filesystem::local::create_directory(path)
}

use futures_util::future::BoxFuture;
use russh_sftp::client::SftpSession;

use crate::filesystem::{self, Entry};

use super::Direction;

fn discover_remote_tree<'a>(
    sftp: &'a SftpSession, root: &'a str, cancel: &'a AtomicBool,
) -> BoxFuture<'a, Result<Option<DiscoveredTree>>> {
    Box::pin(async move {
        let mut tree = DiscoveredTree { directories: Vec::new(), files: Vec::new(), skipped_symlinks: 0 };
        match discover_remote_tree_into(sftp, root, Path::new(""), &mut tree, cancel).await? {
            Walk::Completed => Ok(Some(tree)),
            Walk::Cancelled => Ok(None),
        }
    })
}

fn discover_remote_tree_into<'a>(
    sftp: &'a SftpSession, root: &'a str, relative: &'a Path, tree: &'a mut DiscoveredTree, cancel: &'a AtomicBool,
) -> BoxFuture<'a, Result<Walk>> {
    Box::pin(async move {
        let current = if relative.as_os_str().is_empty() {
            root.to_string()
        } else {
            filesystem::remote::join(root, &relative.to_string_lossy())
        };

        for dir_entry in sftp.read_dir(&current).await? {
            if cancel.load(Ordering::Relaxed) {
                return Ok(Walk::Cancelled);
            }
            let metadata = dir_entry.metadata();
            let entry_relative = relative.join(dir_entry.file_name());

            if metadata.is_symlink() {
                tree.skipped_symlinks += 1;
                continue;
            }

            if metadata.is_dir() {
                tree.directories.push(entry_relative.clone());
                if let Walk::Cancelled = discover_remote_tree_into(sftp, root, &entry_relative, tree, cancel).await? {
                    return Ok(Walk::Cancelled);
                }
            } else {
                tree.files.push((entry_relative, metadata.len()));
            }
        }
        Ok(Walk::Completed)
    })
}

async fn ensure_remote_directory(sftp: &SftpSession, path: &str) -> Result<()> {
    if sftp.metadata(path).await.is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    filesystem::remote::create_directory(sftp, path).await
}

fn planned_file_for_loose_entry(direction: Direction, entry: &Entry, dest_dir: &Path) -> PlannedFile {
    let (local_path, remote_path) = match direction {
        Direction::Upload => (entry.path.clone(), filesystem::path_to_remote_string(&dest_dir.join(&entry.name))),
        Direction::Download => (dest_dir.join(&entry.name), filesystem::path_to_remote_string(&entry.path)),
    };
    PlannedFile { local_path, remote_path, display_name: entry.name.clone(), size: entry.size }
}

fn planned_files_for_tree(
    direction: Direction, entry: &Entry, dest_root: &Path, tree: &DiscoveredTree,
) -> Vec<PlannedFile> {
    tree.files
        .iter()
        .map(|(relative_file, size)| {
            let (local_path, remote_path) = match direction {
                Direction::Upload => {
                    (entry.path.join(relative_file), filesystem::path_to_remote_string(&dest_root.join(relative_file)))
                }
                Direction::Download => {
                    (dest_root.join(relative_file), filesystem::path_to_remote_string(&entry.path.join(relative_file)))
                }
            };
            PlannedFile {
                local_path,
                remote_path,
                display_name: relative_file.to_string_lossy().into_owned(),
                size: *size,
            }
        })
        .collect()
}

pub async fn plan_directory_copy(
    direction: Direction, source_entries: Vec<Entry>, dest_dir: &Path, sftp: &SftpSession, cancel: &AtomicBool,
) -> Result<PlanOutcome> {
    let mut files = Vec::new();
    let mut skipped_symlinks = 0;

    for entry in source_entries {
        if cancel.load(Ordering::Relaxed) {
            return Ok(PlanOutcome::Cancelled);
        }
        if !entry.is_dir {
            files.push(planned_file_for_loose_entry(direction, &entry, dest_dir));
            continue;
        }

        let tree = match direction {
            Direction::Upload => discover_local_tree(&entry.path, cancel)?,
            Direction::Download => {
                let root = filesystem::path_to_remote_string(&entry.path);
                discover_remote_tree(sftp, &root, cancel).await?
            }
        };
        let Some(tree) = tree else {
            return Ok(PlanOutcome::Cancelled);
        };
        skipped_symlinks += tree.skipped_symlinks;

        let dest_root = dest_dir.join(&entry.name);
        let directories =
            std::iter::once(dest_root.clone()).chain(tree.directories.iter().map(|relative| dest_root.join(relative)));
        for directory in directories {
            if cancel.load(Ordering::Relaxed) {
                return Ok(PlanOutcome::Cancelled);
            }
            ensure_directory(direction, sftp, &directory).await?;
        }

        files.extend(planned_files_for_tree(direction, &entry, &dest_root, &tree));
    }

    Ok(PlanOutcome::Ready(DirectoryPlan { files, skipped_symlinks }))
}

async fn ensure_directory(direction: Direction, sftp: &SftpSession, path: &Path) -> Result<()> {
    match direction {
        Direction::Upload => {
            let remote = filesystem::path_to_remote_string(path);
            ensure_remote_directory(sftp, &remote).await
        }
        Direction::Download => ensure_local_directory(path),
    }
}

#[cfg(test)]
#[path = "../../tests/transfer/plan_test.rs"]
mod tests;
