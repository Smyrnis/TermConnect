use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExistingFile {
    pub size: u64,
    pub modified: Option<u64>,
    pub is_dir: bool,
}

pub type DestinationListing = HashMap<String, ExistingFile>;

pub struct PlannedFile {
    pub local_path: PathBuf,
    pub remote_path: String,
    pub display_name: String,
    pub size: u64,
    pub existing: Option<ExistingFile>,
    pub source_modified: Option<u64>,
    pub partial: Option<ExistingFile>,
    pub resume: bool,
}

impl PlannedFile {
    pub fn is_conflict(&self) -> bool {
        self.existing.is_some() || self.partial.is_some()
    }

    pub fn destination(&self, direction: Direction) -> PathBuf {
        match direction {
            Direction::Upload => PathBuf::from(&self.remote_path),
            Direction::Download => self.local_path.clone(),
        }
    }
}

pub struct DirectoryPlan {
    pub files: Vec<PlannedFile>,
    pub skipped_symlinks: usize,
    pub taken_names: HashMap<PathBuf, HashSet<String>>,
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
    files: Vec<(PathBuf, u64, Option<u64>)>,
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
            let metadata = dir_entry.metadata()?;
            tree.files.push((entry_relative, metadata.len(), unix_seconds(metadata.modified().ok())));
        }
    }
    Ok(Walk::Completed)
}

fn ensure_local_directory(path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    crate::filesystem::local::create_directory(path)?;
    Ok(true)
}

use futures_util::future::BoxFuture;
use russh_sftp::client::SftpSession;

use super::Direction;
use crate::filesystem::{self, Entry};

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
                tree.files.push((entry_relative, metadata.len(), metadata.mtime.map(u64::from)));
            }
        }
        Ok(Walk::Completed)
    })
}

async fn ensure_remote_directory(sftp: &SftpSession, path: &str) -> Result<bool> {
    if sftp.metadata(path).await.is_ok_and(|m| m.is_dir()) {
        return Ok(false);
    }
    filesystem::remote::create_directory(sftp, path).await?;
    Ok(true)
}

fn planned_file_for_loose_entry(direction: Direction, entry: &Entry, dest_dir: &Path) -> PlannedFile {
    let (local_path, remote_path) = match direction {
        Direction::Upload => (entry.path.clone(), filesystem::path_to_remote_string(&dest_dir.join(&entry.name))),
        Direction::Download => (dest_dir.join(&entry.name), filesystem::path_to_remote_string(&entry.path)),
    };
    PlannedFile {
        local_path,
        remote_path,
        display_name: entry.name.clone(),
        size: entry.size,
        existing: None,
        source_modified: None,
        partial: None,
        resume: false,
    }
}

fn planned_files_for_tree(
    direction: Direction, entry: &Entry, dest_root: &Path, tree: &DiscoveredTree,
) -> Vec<PlannedFile> {
    tree.files
        .iter()
        .map(|(relative_file, size, modified)| {
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
                existing: None,
                source_modified: *modified,
                partial: None,
                resume: false,
            }
        })
        .collect()
}

pub async fn plan_copy(
    direction: Direction, source_entries: Vec<Entry>, dest_dir: &Path, sftp: &SftpSession, cancel: &AtomicBool,
) -> Result<PlanOutcome> {
    let mut files = Vec::new();
    let mut skipped_symlinks = 0;
    let mut created: HashSet<PathBuf> = HashSet::new();

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
            if ensure_directory(direction, sftp, &directory).await? {
                created.insert(directory);
            }
        }

        files.extend(planned_files_for_tree(direction, &entry, &dest_root, &tree));
    }

    let names_by_parent = names_by_parent(direction, &files);
    let mut listings = HashMap::new();
    for parent in parents_needing_listing(direction, &files, &created) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(PlanOutcome::Cancelled);
        }
        let listing = match direction {
            Direction::Upload => {
                list_remote_destination(
                    sftp,
                    &filesystem::path_to_remote_string(&parent),
                    names_by_parent.get(&parent).map(Vec::as_slice).unwrap_or_default(),
                )
                .await?
            }
            Direction::Download => list_local_destination(&parent)?,
        };
        listings.insert(parent, listing);
    }
    mark_conflicts(direction, &mut files, &listings);
    for file in files.iter_mut().filter(|file| file.is_conflict() && file.source_modified.is_none()) {
        file.source_modified = source_modified(direction, sftp, file).await;
    }
    let taken_names = taken_names(direction, &files, &listings);

    Ok(PlanOutcome::Ready(DirectoryPlan { files, skipped_symlinks, taken_names }))
}

fn destination_parent_and_name(direction: Direction, file: &PlannedFile) -> Option<(PathBuf, String)> {
    let destination = file.destination(direction);
    Some((destination.parent()?.to_path_buf(), destination.file_name()?.to_string_lossy().into_owned()))
}

pub(crate) fn parents_needing_listing(
    direction: Direction, files: &[PlannedFile], created: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = files
        .iter()
        .filter_map(|file| destination_parent_and_name(direction, file))
        .map(|(parent, _)| parent)
        .filter(|parent| !created.contains(parent))
        .collect();
    parents.sort();
    parents.dedup();
    parents
}

fn names_by_parent(direction: Direction, files: &[PlannedFile]) -> HashMap<PathBuf, Vec<String>> {
    let mut names: HashMap<PathBuf, Vec<String>> = HashMap::new();
    for (parent, name) in files.iter().filter_map(|file| destination_parent_and_name(direction, file)) {
        names.entry(parent).or_default().push(name);
    }
    names
}

pub(crate) fn mark_conflicts(
    direction: Direction, files: &mut [PlannedFile], listings: &HashMap<PathBuf, DestinationListing>,
) {
    for file in files.iter_mut() {
        let Some((parent, name)) = destination_parent_and_name(direction, file) else {
            continue;
        };
        file.existing = listings.get(&parent).and_then(|listing| listing.get(&name)).copied();
        file.partial = listings
            .get(&parent)
            .and_then(|listing| listing.get(&format!("{name}.part")))
            .copied()
            .filter(|partial| !partial.is_dir);
    }
}

pub(crate) fn taken_names(
    direction: Direction, files: &[PlannedFile], listings: &HashMap<PathBuf, DestinationListing>,
) -> HashMap<PathBuf, HashSet<String>> {
    let mut taken: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for (parent, listing) in listings {
        taken.entry(parent.clone()).or_default().extend(listing.keys().cloned());
    }
    for (parent, name) in files.iter().filter_map(|file| destination_parent_and_name(direction, file)) {
        taken.entry(parent).or_default().insert(name);
    }
    taken
}

pub(crate) fn list_local_destination(path: &Path) -> Result<DestinationListing> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(err) => return Err(err.into()),
    };
    let mut listing = HashMap::new();
    for entry in entries.flatten() {
        let is_symlink = entry.file_type().is_ok_and(|file_type| file_type.is_symlink());
        let metadata =
            if is_symlink { fs::metadata(entry.path()).or_else(|_| entry.metadata()) } else { entry.metadata() };
        let existing = match metadata {
            Ok(metadata) => ExistingFile {
                size: metadata.len(),
                modified: unix_seconds(metadata.modified().ok()),
                is_dir: metadata.is_dir(),
            },
            Err(_) => ExistingFile {
                size: 0,
                modified: None,
                is_dir: entry.file_type().is_ok_and(|file_type| file_type.is_dir()),
            },
        };
        listing.insert(entry.file_name().to_string_lossy().into_owned(), existing);
    }
    Ok(listing)
}

async fn list_remote_destination(
    sftp: &SftpSession, path: &str, planned_names: &[String],
) -> Result<DestinationListing> {
    let mut listing = HashMap::new();
    match sftp.read_dir(path).await {
        Ok(entries) => {
            for entry in entries {
                let name = entry.file_name();
                let metadata = entry.metadata();
                let existing = if metadata.is_symlink() {
                    match sftp.metadata(filesystem::remote::join(path, &name)).await {
                        Ok(target) => existing_from_remote(&target),
                        Err(_) => existing_from_remote(&metadata),
                    }
                } else {
                    existing_from_remote(&metadata)
                };
                listing.insert(name, existing);
            }
        }
        Err(_) if sftp.metadata(path).await.is_err() => {}
        Err(_) => {
            for name in planned_names {
                if let Ok(metadata) = sftp.metadata(filesystem::remote::join(path, name)).await {
                    listing.insert(name.clone(), existing_from_remote(&metadata));
                }
            }
        }
    }
    Ok(listing)
}

fn existing_from_remote(metadata: &russh_sftp::client::fs::Metadata) -> ExistingFile {
    ExistingFile { size: metadata.len(), modified: metadata.mtime.map(u64::from), is_dir: metadata.is_dir() }
}

async fn source_modified(direction: Direction, sftp: &SftpSession, file: &PlannedFile) -> Option<u64> {
    match direction {
        Direction::Upload => unix_seconds(fs::metadata(&file.local_path).ok()?.modified().ok()),
        Direction::Download => sftp.metadata(&file.remote_path).await.ok()?.mtime.map(u64::from),
    }
}

pub(crate) fn unix_seconds(time: Option<SystemTime>) -> Option<u64> {
    time?.duration_since(UNIX_EPOCH).ok().map(|elapsed| elapsed.as_secs())
}

async fn ensure_directory(direction: Direction, sftp: &SftpSession, path: &Path) -> Result<bool> {
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
