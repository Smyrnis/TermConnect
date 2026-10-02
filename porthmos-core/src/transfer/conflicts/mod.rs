use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use super::plan::{DirectoryPlan, ExistingFile, PlannedFile};

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
    Resume,
}

impl ConflictPolicy {
    pub fn resolution_for(self, file: &PlannedFile) -> Option<Resolution> {
        match (self, file.existing) {
            (ConflictPolicy::Ask, _) => None,
            (_, None) if file.partial.is_some_and(|partial| partial.size >= file.size) => Some(Resolution::Overwrite),
            (_, None) => Some(Resolution::Resume),
            (ConflictPolicy::Overwrite, Some(_)) if file.partial.is_some() => Some(Resolution::Resume),
            (ConflictPolicy::Overwrite, Some(_)) => Some(Resolution::Overwrite),
            (ConflictPolicy::Skip, Some(_)) => Some(Resolution::Skip),
            (ConflictPolicy::Rename, Some(_)) => Some(Resolution::Rename),
        }
    }
}

pub fn unique_name(name: &str, taken: &HashSet<String>) -> String {
    let (stem, extension) = split_extension(name);
    (1..)
        .map(|counter| format!("{stem} ({counter}){extension}"))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_else(|| name.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    pub display_name: String,
    pub existing: Option<ExistingFile>,
    pub partial: Option<ExistingFile>,
    pub new_size: u64,
    pub new_modified: Option<u64>,
}

impl From<&PlannedFile> for ConflictInfo {
    fn from(file: &PlannedFile) -> Self {
        Self {
            display_name: file.display_name.clone(),
            existing: file.existing,
            partial: file.partial,
            new_size: file.size,
            new_modified: file.source_modified,
        }
    }
}

pub trait ConflictSides {
    fn existing(&self) -> Option<ExistingFile>;
    fn partial(&self) -> Option<ExistingFile>;
}

impl ConflictSides for PlannedFile {
    fn existing(&self) -> Option<ExistingFile> {
        self.existing
    }

    fn partial(&self) -> Option<ExistingFile> {
        self.partial
    }
}

impl ConflictSides for ConflictInfo {
    fn existing(&self) -> Option<ExistingFile> {
        self.existing
    }

    fn partial(&self) -> Option<ExistingFile> {
        self.partial
    }
}

pub fn fits_the_rest<T: ConflictSides>(resolution: Resolution, answered: &T, later: &T) -> bool {
    let later_blocked_by_folder = later.existing().is_some_and(|existing| existing.is_dir);
    match resolution {
        Resolution::Resume => later.partial().is_some() && !later_blocked_by_folder,
        Resolution::Overwrite => later.existing().is_some() == answered.existing().is_some(),
        Resolution::Rename => later.existing().is_some(),
        Resolution::Skip => true,
    }
}

pub fn conflict_indices(plan: &DirectoryPlan) -> Vec<usize> {
    plan.files.iter().enumerate().filter(|(_, file)| file.is_conflict()).map(|(index, _)| index).collect()
}

pub struct ResolvedPlan {
    pub files: Vec<PlannedFile>,
    pub skipped: usize,
    pub skipped_partials: usize,
    pub blocked_by_folder: usize,
}

pub fn resolve(plan: DirectoryPlan, answers: &[Resolution]) -> ResolvedPlan {
    let DirectoryPlan { files, mut taken_names, .. } = plan;
    let mut answers = answers.iter().copied();
    let mut resolved = Vec::new();
    let mut skipped = 0;
    let mut skipped_partials = 0;
    let mut blocked_by_folder = 0;
    for mut file in files {
        if !file.is_conflict() {
            resolved.push(file);
            continue;
        }
        let blocked = file.existing.is_some_and(|existing| existing.is_dir);
        match answers.next().unwrap_or(Resolution::Skip) {
            Resolution::Overwrite | Resolution::Resume if blocked => blocked_by_folder += 1,
            Resolution::Resume => {
                file.resume = true;
                resolved.push(file);
            }
            Resolution::Overwrite => {
                file.resume = false;
                resolved.push(file);
            }
            Resolution::Skip if file.existing.is_some() => skipped += 1,
            Resolution::Skip => skipped_partials += 1,
            Resolution::Rename if file.existing.is_none() => {
                file.resume = true;
                resolved.push(file);
            }
            Resolution::Rename => {
                rename_destination(&mut file, &mut taken_names);
                file.resume = false;
                resolved.push(file);
            }
        }
    }
    ResolvedPlan { files: resolved, skipped, skipped_partials, blocked_by_folder }
}

fn rename_destination(file: &mut PlannedFile, taken_names: &mut HashMap<PathBuf, HashSet<String>>) {
    let (Some(parent), Some(name)) = (file.destination.parent(), file.destination.file_name()) else {
        return;
    };
    let parent = parent.to_path_buf();
    let taken = taken_names.entry(parent.clone()).or_default();
    let new_name = unique_name(&name.to_string_lossy(), taken);
    taken.insert(new_name.clone());
    file.destination = parent.join(&new_name);
    file.display_name = Path::new(&file.display_name).with_file_name(&new_name).to_string_lossy().into_owned();
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(index) => (&name[..index], &name[index..]),
    }
}

#[cfg(test)]
mod tests;
