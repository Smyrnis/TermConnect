use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use super::{
    Direction,
    plan::{DirectoryPlan, PlannedFile},
};
use crate::filesystem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Ask,
    Overwrite,
    Skip,
    Rename,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Overwrite,
    Skip,
    Rename,
}

impl ConflictPolicy {
    pub fn automatic_resolution(self) -> Option<Resolution> {
        match self {
            ConflictPolicy::Ask => None,
            ConflictPolicy::Overwrite => Some(Resolution::Overwrite),
            ConflictPolicy::Skip => Some(Resolution::Skip),
            ConflictPolicy::Rename => Some(Resolution::Rename),
        }
    }
}

pub fn unique_name(name: &str, taken: &HashSet<String>) -> String {
    let (stem, extension) = split_extension(name);
    (1..).map(|counter| format!("{stem} ({counter}){extension}")).find(|candidate| !taken.contains(candidate)).unwrap_or_else(|| name.to_string())
}

pub fn conflict_indices(plan: &DirectoryPlan) -> Vec<usize> {
    plan.files.iter().enumerate().filter(|(_, file)| file.existing.is_some()).map(|(index, _)| index).collect()
}

pub struct ResolvedPlan {
    pub files: Vec<PlannedFile>,
    pub skipped: usize,
    pub blocked_by_folder: usize,
}

pub fn resolve(plan: DirectoryPlan, answers: &[Resolution], direction: Direction) -> ResolvedPlan {
    let DirectoryPlan { files, mut taken_names, .. } = plan;
    let mut answers = answers.iter().copied();
    let mut resolved = Vec::new();
    let mut skipped = 0;
    let mut blocked_by_folder = 0;
    for mut file in files {
        let Some(existing) = file.existing else {
            resolved.push(file);
            continue;
        };
        match answers.next().unwrap_or(Resolution::Skip) {
            Resolution::Overwrite if !existing.is_dir => resolved.push(file),
            Resolution::Overwrite => blocked_by_folder += 1,
            Resolution::Skip => skipped += 1,
            Resolution::Rename => {
                rename_destination(direction, &mut file, &mut taken_names);
                resolved.push(file);
            }
        }
    }
    ResolvedPlan { files: resolved, skipped, blocked_by_folder }
}

fn rename_destination(direction: Direction, file: &mut PlannedFile, taken_names: &mut HashMap<PathBuf, HashSet<String>>) {
    let destination = file.destination(direction);
    let (Some(parent), Some(name)) = (destination.parent(), destination.file_name()) else {
        return;
    };
    let taken = taken_names.entry(parent.to_path_buf()).or_default();
    let new_name = unique_name(&name.to_string_lossy(), taken);
    taken.insert(new_name.clone());
    let new_destination = parent.join(&new_name);
    match direction {
        Direction::Upload => file.remote_path = filesystem::path_to_remote_string(&new_destination),
        Direction::Download => file.local_path = new_destination,
    }
    file.display_name = Path::new(&file.display_name).with_file_name(&new_name).to_string_lossy().into_owned();
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(index) => (&name[..index], &name[index..]),
    }
}

#[cfg(test)]
#[path = "../../tests/transfer/conflicts_test.rs"]
mod tests;
