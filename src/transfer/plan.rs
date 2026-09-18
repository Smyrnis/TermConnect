use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// One file discovered while planning a directory copy, with its final
/// source and destination already resolved — ready to enqueue as an
/// ordinary `TransferJob`.
pub struct PlannedFile {
    pub local_path: PathBuf,
    pub remote_path: String,
    pub display_name: String,
    pub size: u64,
}

/// The result of planning one directory copy: every file to transfer,
/// plus how many symlinks were skipped along the way.
pub struct DirectoryPlan {
    pub files: Vec<PlannedFile>,
    pub skipped_symlinks: usize,
}

/// A tree discovered by walking a source directory, independent of where
/// it will be copied to. `directories` and `files` hold paths relative to
/// the walked root; `directories` is ordered parent-before-child, so
/// creating them at a destination in list order never tries to create a
/// child before its parent exists.
struct DiscoveredTree {
    directories: Vec<PathBuf>,
    files: Vec<(PathBuf, u64)>,
    skipped_symlinks: usize,
}

/// Recursively lists everything under `root` (a local directory), never
/// following symlinks — a symlink is counted in `skipped_symlinks` and
/// otherwise ignored, which also means a symlink back to an ancestor
/// directory can never cause infinite recursion.
fn discover_local_tree(root: &Path) -> Result<DiscoveredTree> {
    let mut tree = DiscoveredTree {
        directories: Vec::new(),
        files: Vec::new(),
        skipped_symlinks: 0,
    };
    discover_local_tree_into(root, Path::new(""), &mut tree)?;
    Ok(tree)
}

fn discover_local_tree_into(root: &Path, relative: &Path, tree: &mut DiscoveredTree) -> Result<()> {
    for dir_entry in fs::read_dir(root.join(relative))? {
        let dir_entry = dir_entry?;
        let file_type = dir_entry.file_type()?;
        let entry_relative = relative.join(dir_entry.file_name());

        if file_type.is_symlink() {
            tree.skipped_symlinks += 1;
            continue;
        }

        if file_type.is_dir() {
            tree.directories.push(entry_relative.clone());
            discover_local_tree_into(root, &entry_relative, tree)?;
        } else {
            let size = dir_entry.metadata()?.len();
            tree.files.push((entry_relative, size));
        }
    }
    Ok(())
}

/// Creates `path` if it doesn't already exist. A directory that's already
/// there (e.g. copying into a destination that partially exists) is left
/// as is, not an error — a directory copy merges into an existing
/// destination rather than failing on it.
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

/// The async counterpart to `discover_local_tree_into` — recurses over an
/// SFTP directory listing instead of a local one. Boxed because async fns
/// can't recurse directly (mirrors `filesystem::remote::remove_dir_recursive`,
/// the existing recursive-SFTP-walk pattern in this codebase).
fn discover_remote_tree<'a>(
    sftp: &'a SftpSession,
    root: &'a str,
) -> BoxFuture<'a, Result<DiscoveredTree>> {
    Box::pin(async move {
        let mut tree = DiscoveredTree {
            directories: Vec::new(),
            files: Vec::new(),
            skipped_symlinks: 0,
        };
        discover_remote_tree_into(sftp, root, Path::new(""), &mut tree).await?;
        Ok(tree)
    })
}

fn discover_remote_tree_into<'a>(
    sftp: &'a SftpSession,
    root: &'a str,
    relative: &'a Path,
    tree: &'a mut DiscoveredTree,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let current = if relative.as_os_str().is_empty() {
            root.to_string()
        } else {
            filesystem::remote::join(root, &relative.to_string_lossy())
        };

        for dir_entry in sftp.read_dir(&current).await? {
            let metadata = dir_entry.metadata();
            let entry_relative = relative.join(dir_entry.file_name());

            if metadata.is_symlink() {
                tree.skipped_symlinks += 1;
                continue;
            }

            if metadata.is_dir() {
                tree.directories.push(entry_relative.clone());
                discover_remote_tree_into(sftp, root, &entry_relative, tree).await?;
            } else {
                tree.files.push((entry_relative, metadata.len()));
            }
        }
        Ok(())
    })
}

/// Creates the remote directory `path` if it doesn't already exist —
/// checked with `metadata` first rather than inspecting the error from a
/// failed `create_dir`, since the base SFTP v3 status codes this crate
/// exposes have no distinct "already exists" code (mkdir-on-existing-dir
/// and other server-side failures both come back as the same generic
/// `Failure` status), so error-based detection can't reliably tell them
/// apart. Only treats an existing *directory* as "already there" — a
/// regular file already sitting at `path` (a name collision) falls
/// through to `create_directory`, which then fails loudly with a real,
/// surfaced error instead of planning silently succeeding over a file.
async fn ensure_remote_directory(sftp: &SftpSession, path: &str) -> Result<()> {
    if sftp.metadata(path).await.is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    filesystem::remote::create_directory(sftp, path).await
}

/// Resolves a loose file's (not under any directory) final source and
/// destination paths — pure path mapping, no I/O. Split out of
/// `plan_directory_copy` so it's unit-testable without a live
/// `SftpSession`.
fn planned_file_for_loose_entry(
    direction: Direction,
    entry: &Entry,
    dest_dir: &Path,
) -> PlannedFile {
    let (local_path, remote_path) = match direction {
        Direction::Upload => (
            entry.path.clone(),
            filesystem::path_to_remote_string(&dest_dir.join(&entry.name)),
        ),
        Direction::Download => (
            dest_dir.join(&entry.name),
            filesystem::path_to_remote_string(&entry.path),
        ),
    };
    PlannedFile {
        local_path,
        remote_path,
        display_name: entry.name.clone(),
        size: entry.size,
    }
}

/// Resolves the final source/destination paths for every file already
/// discovered under a walked directory (`tree`) — pure path mapping, no
/// I/O. Split out of `plan_directory_copy` so it's unit-testable without a
/// live `SftpSession`.
fn planned_files_for_tree(
    direction: Direction,
    entry: &Entry,
    dest_root: &Path,
    tree: &DiscoveredTree,
) -> Vec<PlannedFile> {
    tree.files
        .iter()
        .map(|(relative_file, size)| {
            let (local_path, remote_path) = match direction {
                Direction::Upload => (
                    entry.path.join(relative_file),
                    filesystem::path_to_remote_string(&dest_root.join(relative_file)),
                ),
                Direction::Download => (
                    dest_root.join(relative_file),
                    filesystem::path_to_remote_string(&entry.path.join(relative_file)),
                ),
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

/// Walks `source_entries` (an `F5` selection that contains at least one
/// directory) and creates the matching destination structure under
/// `dest_dir`, on whichever side is remote for `direction`. Returns every
/// file found, ready to enqueue — loose files in the selection (not under
/// any directory) pass straight through unchanged, alongside every file
/// discovered under each selected directory.
pub async fn plan_directory_copy(
    direction: Direction,
    source_entries: Vec<Entry>,
    dest_dir: &Path,
    sftp: &SftpSession,
) -> Result<DirectoryPlan> {
    let mut files = Vec::new();
    let mut skipped_symlinks = 0;

    for entry in source_entries {
        if !entry.is_dir {
            files.push(planned_file_for_loose_entry(direction, &entry, dest_dir));
            continue;
        }

        let tree = match direction {
            Direction::Upload => discover_local_tree(&entry.path)?,
            Direction::Download => {
                let root = filesystem::path_to_remote_string(&entry.path);
                discover_remote_tree(sftp, &root).await?
            }
        };
        skipped_symlinks += tree.skipped_symlinks;

        let dest_root = dest_dir.join(&entry.name);
        ensure_directory(direction, sftp, &dest_root).await?;
        for relative_dir in &tree.directories {
            ensure_directory(direction, sftp, &dest_root.join(relative_dir)).await?;
        }

        files.extend(planned_files_for_tree(direction, &entry, &dest_root, &tree));
    }

    Ok(DirectoryPlan {
        files,
        skipped_symlinks,
    })
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
