use std::{path::Path, sync::atomic::AtomicBool};

use porthmos_vfs::{FileSystem, ProtocolError};

use super::{Tree, TreeEntry, TreeKind};
use crate::transfer::plan::discover;

pub struct Scanned {
    pub tree: Tree,
    pub skipped_symlinks: usize,
}

pub async fn scan_tree(
    fs: &dyn FileSystem, root: &Path, subfolders: bool, cancel: &AtomicBool,
) -> Result<Option<Scanned>, ProtocolError> {
    let Some(found) = discover(fs, root, subfolders, cancel).await? else {
        return Ok(None);
    };
    let mut tree = Tree::new();
    for directory in found.directories {
        tree.insert(directory, TreeEntry { kind: TreeKind::Dir, size: 0, modified: None });
    }
    for (path, size, modified) in found.files {
        tree.insert(path, TreeEntry { kind: TreeKind::File, size, modified });
    }
    Ok(Some(Scanned { tree, skipped_symlinks: found.skipped_symlinks }))
}

pub async fn scan_both(
    local: &dyn FileSystem, local_root: &Path, remote: &dyn FileSystem, remote_root: &Path, subfolders: bool,
    cancel: &AtomicBool,
) -> Result<Option<(Scanned, Scanned)>, ProtocolError> {
    let (local_scan, remote_scan) = tokio::try_join!(
        scan_tree(local, local_root, subfolders, cancel),
        scan_tree(remote, remote_root, subfolders, cancel)
    )?;
    Ok(match (local_scan, remote_scan) {
        (Some(left), Some(right)) => Some((left, right)),
        _ => None,
    })
}

#[cfg(test)]
mod tests;
