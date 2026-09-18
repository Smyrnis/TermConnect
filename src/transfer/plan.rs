use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// One file discovered while planning a directory copy, with its final
/// source and destination already resolved — ready to enqueue as an
/// ordinary `TransferJob`.
#[allow(dead_code)]
pub struct PlannedFile {
    pub local_path: PathBuf,
    pub remote_path: String,
    pub display_name: String,
    pub size: u64,
}

/// The result of planning one directory copy: every file to transfer,
/// plus how many symlinks were skipped along the way.
#[allow(dead_code)]
pub struct DirectoryPlan {
    pub files: Vec<PlannedFile>,
    pub skipped_symlinks: usize,
}

/// A tree discovered by walking a source directory, independent of where
/// it will be copied to. `directories` and `files` hold paths relative to
/// the walked root; `directories` is ordered parent-before-child, so
/// creating them at a destination in list order never tries to create a
/// child before its parent exists.
#[allow(dead_code)]
struct DiscoveredTree {
    directories: Vec<PathBuf>,
    files: Vec<(PathBuf, u64)>,
    skipped_symlinks: usize,
}

/// Recursively lists everything under `root` (a local directory), never
/// following symlinks — a symlink is counted in `skipped_symlinks` and
/// otherwise ignored, which also means a symlink back to an ancestor
/// directory can never cause infinite recursion.
#[allow(dead_code)]
fn discover_local_tree(root: &Path) -> Result<DiscoveredTree> {
    let mut tree = DiscoveredTree {
        directories: Vec::new(),
        files: Vec::new(),
        skipped_symlinks: 0,
    };
    discover_local_tree_into(root, Path::new(""), &mut tree)?;
    Ok(tree)
}

#[allow(dead_code)]
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
#[allow(dead_code)]
fn ensure_local_directory(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    crate::filesystem::local::create_directory(path)
}

#[cfg(test)]
#[path = "../../tests/transfer/plan_test.rs"]
mod tests;
