use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::PathBuf,
};

pub mod scan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    LocalToRemote,
    RemoteToLocal,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncBy {
    Time,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncOptions {
    pub direction: SyncDirection,
    pub by: SyncBy,
    pub subfolders: bool,
}

impl SyncOptions {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.direction == SyncDirection::Both && self.by == SyncBy::Size {
            return Err("Comparing by size can't decide a direction, so it can't be used with Both");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    Upload,
    Download,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncReason {
    OnlyLocal,
    OnlyRemote,
    LocalNewer,
    RemoteNewer,
    TargetNewer,
    SameTimeDifferentSize,
    SizeDiffers,
    SizeDiffersTimeUnknown,
    KindMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncFacts {
    pub size: u64,
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncItem {
    pub id: u32,
    pub path: PathBuf,
    pub local: Option<SyncFacts>,
    pub remote: Option<SyncFacts>,
    pub action: SyncAction,
    pub reason: SyncReason,
    pub ticked: bool,
    pub flippable: bool,
}

impl SyncItem {
    pub fn allowed_actions(&self, direction: SyncDirection) -> Vec<SyncAction> {
        if self.reason == SyncReason::KindMismatch {
            return vec![SyncAction::Skip];
        }
        let upload = self.local.is_some() && direction != SyncDirection::RemoteToLocal;
        let download = self.remote.is_some() && direction != SyncDirection::LocalToRemote;
        let mut allowed = vec![SyncAction::Skip];
        if upload {
            allowed.push(SyncAction::Upload);
        }
        if download {
            allowed.push(SyncAction::Download);
        }
        allowed
    }

    pub fn allows(&self, action: SyncAction, direction: SyncDirection) -> bool {
        self.allowed_actions(direction).contains(&action)
    }

    pub fn next_action(&self, direction: SyncDirection) -> SyncAction {
        self.action_after(self.action, direction)
    }

    pub fn action_after(&self, current: SyncAction, direction: SyncDirection) -> SyncAction {
        let allowed = self.allowed_actions(direction);
        match allowed.iter().position(|action| *action == current) {
            Some(position) => allowed[(position + 1) % allowed.len()],
            None => SyncAction::Skip,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    pub sync_id: u64,
    pub session: u64,
    pub local_root: PathBuf,
    pub remote_root: PathBuf,
    pub options: SyncOptions,
    pub items: Vec<SyncItem>,
    pub skipped_symlinks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKind {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeEntry {
    pub kind: TreeKind,
    pub size: u64,
    pub modified: Option<u64>,
}

impl TreeEntry {
    fn facts(self) -> SyncFacts {
        SyncFacts { size: self.size, modified: self.modified }
    }
}

pub type Tree = BTreeMap<PathBuf, TreeEntry>;

struct Verdict {
    action: SyncAction,
    reason: SyncReason,
    ticked: bool,
}

fn copy(action: SyncAction, reason: SyncReason) -> Option<Verdict> {
    Some(Verdict { action, reason, ticked: true })
}

fn ask(reason: SyncReason) -> Option<Verdict> {
    Some(Verdict { action: SyncAction::Skip, reason, ticked: false })
}

fn only_one_side(on_local: bool, direction: SyncDirection) -> Option<Verdict> {
    match (on_local, direction) {
        (true, SyncDirection::LocalToRemote | SyncDirection::Both) => copy(SyncAction::Upload, SyncReason::OnlyLocal),
        (false, SyncDirection::RemoteToLocal | SyncDirection::Both) => {
            copy(SyncAction::Download, SyncReason::OnlyRemote)
        }
        _ => None,
    }
}

fn newer_side(local_is_newer: bool, direction: SyncDirection) -> Option<Verdict> {
    match (local_is_newer, direction) {
        (true, SyncDirection::LocalToRemote | SyncDirection::Both) => copy(SyncAction::Upload, SyncReason::LocalNewer),
        (false, SyncDirection::RemoteToLocal | SyncDirection::Both) => {
            copy(SyncAction::Download, SyncReason::RemoteNewer)
        }
        _ => ask(SyncReason::TargetNewer),
    }
}

fn by_size(local: TreeEntry, remote: TreeEntry, direction: SyncDirection) -> Option<Verdict> {
    if local.size == remote.size {
        return None;
    }
    match direction {
        SyncDirection::LocalToRemote => copy(SyncAction::Upload, SyncReason::SizeDiffers),
        SyncDirection::RemoteToLocal => copy(SyncAction::Download, SyncReason::SizeDiffers),
        SyncDirection::Both => ask(SyncReason::SizeDiffers),
    }
}

fn by_time(
    local: TreeEntry, remote: TreeEntry, direction: SyncDirection, tolerance: u64, target_keeps_times: bool,
) -> Option<Verdict> {
    let sizes_equal = local.size == remote.size;
    let (Some(local_time), Some(remote_time)) = (local.modified, remote.modified) else {
        if sizes_equal {
            return None;
        }
        return match direction {
            SyncDirection::LocalToRemote => copy(SyncAction::Upload, SyncReason::SizeDiffersTimeUnknown),
            SyncDirection::RemoteToLocal => copy(SyncAction::Download, SyncReason::SizeDiffersTimeUnknown),
            SyncDirection::Both => ask(SyncReason::SizeDiffersTimeUnknown),
        };
    };
    let verdict = if local_time > remote_time.saturating_add(tolerance) {
        newer_side(true, direction)
    } else if remote_time > local_time.saturating_add(tolerance) {
        newer_side(false, direction)
    } else {
        return if sizes_equal { None } else { ask(SyncReason::SameTimeDifferentSize) };
    };
    match verdict {
        Some(found) if found.reason == SyncReason::TargetNewer && sizes_equal && !target_keeps_times => None,
        other => other,
    }
}

fn flippable(verdict: &Verdict) -> bool {
    verdict.action == SyncAction::Skip
        && matches!(
            verdict.reason,
            SyncReason::TargetNewer | SyncReason::SameTimeDifferentSize | SyncReason::SizeDiffersTimeUnknown
        )
}

pub fn compare(
    local: &Tree, remote: &Tree, options: SyncOptions, tolerance: u64, target_keeps_times: bool,
) -> Vec<SyncItem> {
    if options.validate().is_err() {
        return Vec::new();
    }
    let paths: BTreeSet<&PathBuf> = local.keys().chain(remote.keys()).collect();
    let mut mismatched: HashSet<PathBuf> = HashSet::new();
    let mut items = Vec::new();
    for path in paths {
        if path.ancestors().skip(1).any(|ancestor| mismatched.contains(ancestor)) {
            continue;
        }
        let verdict = match (local.get(path), remote.get(path)) {
            (Some(local_entry), Some(remote_entry)) if local_entry.kind != remote_entry.kind => {
                mismatched.insert(path.clone());
                ask(SyncReason::KindMismatch)
            }
            (Some(local_entry), Some(remote_entry)) if local_entry.kind == TreeKind::File => match options.by {
                SyncBy::Size => by_size(*local_entry, *remote_entry, options.direction),
                SyncBy::Time => by_time(*local_entry, *remote_entry, options.direction, tolerance, target_keeps_times),
            },
            (Some(entry), None) if entry.kind == TreeKind::File => only_one_side(true, options.direction),
            (None, Some(entry)) if entry.kind == TreeKind::File => only_one_side(false, options.direction),
            _ => None,
        };
        let Some(verdict) = verdict else {
            continue;
        };
        let can_flip = flippable(&verdict);
        items.push(SyncItem {
            id: items.len() as u32,
            path: path.clone(),
            local: local.get(path).map(|entry| entry.facts()),
            remote: remote.get(path).map(|entry| entry.facts()),
            action: verdict.action,
            reason: verdict.reason,
            ticked: verdict.ticked,
            flippable: can_flip,
        });
    }
    items
}

#[cfg(test)]
mod tests;
