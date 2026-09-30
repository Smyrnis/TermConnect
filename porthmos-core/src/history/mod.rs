use std::{fs, io, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use porthmos_vfs::glob_match;
use serde::{Deserialize, Serialize};

use crate::{Paths, transfer::Direction};

pub const MAX_ENTRIES: usize = 1000;
pub const MAX_FAILED_FILES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryResult {
    Done,
    PartlyFailed { failed: usize },
    Failed,
    Cancelled,
    Interrupted,
}

impl HistoryResult {
    pub fn text(&self) -> &'static str {
        match self {
            HistoryResult::Done => "done",
            HistoryResult::PartlyFailed { .. } => "partly failed",
            HistoryResult::Failed => "failed",
            HistoryResult::Cancelled => "cancelled",
            HistoryResult::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub finished_at: DateTime<Utc>,
    pub connection: String,
    pub direction: Direction,
    pub label: String,
    pub local_path: String,
    pub remote_path: String,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes: u64,
    pub result: HistoryResult,
    #[serde(default)]
    pub failed_count: usize,
    #[serde(default)]
    pub failed_files: Vec<String>,
}

#[derive(Deserialize)]
struct HistoryFile {
    entries: Vec<HistoryEntry>,
}

#[derive(Serialize)]
struct HistoryFileRef<'a> {
    entries: &'a [HistoryEntry],
}

pub struct History {
    path: PathBuf,
    entries: Vec<HistoryEntry>,
    writable: bool,
}

impl History {
    pub fn load(paths: &Paths) -> (Self, Option<String>) {
        let path = paths.history_file();
        let mut history = Self { path: path.clone(), entries: Vec::new(), writable: true };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return (history, None),
            Err(err) => {
                let warning = history.set_aside(&format!("Couldn't read transfer history ({err})"));
                return (history, Some(warning));
            }
        };
        let parsed = String::from_utf8(bytes).ok().and_then(|text| toml::from_str::<HistoryFile>(&text).ok());
        match parsed {
            Some(file) => {
                history.entries = file.entries;
                history.trim();
                (history, None)
            }
            None => {
                let warning = history.set_aside("Transfer history was unreadable");
                (history, Some(warning))
            }
        }
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn newest_first(&self) -> Vec<HistoryEntry> {
        self.entries.iter().rev().cloned().collect()
    }

    pub fn record(&mut self, entry: HistoryEntry) -> Result<()> {
        self.entries.push(entry);
        self.trim();
        self.write()
    }

    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        self.write()
    }

    fn trim(&mut self) {
        let excess = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.drain(..excess);
    }

    fn set_aside(&mut self, problem: &str) -> String {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".broken");
        let broken = PathBuf::from(name);
        match fs::rename(&self.path, &broken) {
            Ok(()) => format!(
                "{problem}; it was kept as {} and a new one was started",
                broken.file_name().and_then(|name| name.to_str()).unwrap_or("history.toml.broken")
            ),
            Err(err) => {
                self.writable = false;
                format!("{problem} and couldn't be set aside ({err}); new entries won't be saved")
            }
        }
    }

    fn write(&self) -> Result<()> {
        if !self.writable {
            anyhow::bail!("the existing history file could not be read or set aside, so it was left alone");
        }
        let text = toml::to_string(&HistoryFileRef { entries: &self.entries })?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("couldn't create {}", parent.display()))?;
        }
        let temp_path = {
            let mut name = self.path.as_os_str().to_owned();
            name.push(".tmp");
            PathBuf::from(name)
        };
        let mut handle = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp_path)
            .with_context(|| format!("couldn't write {}", temp_path.display()))?;
        handle.write_all(text.as_bytes()).with_context(|| format!("couldn't write {}", temp_path.display()))?;
        drop(handle);
        fs::rename(&temp_path, &self.path).with_context(|| format!("couldn't replace {}", self.path.display()))
    }
}

pub fn matches(entry: &HistoryEntry, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let fields = [
        entry.connection.as_str(),
        entry.label.as_str(),
        entry.local_path.as_str(),
        entry.remote_path.as_str(),
        entry.result.text(),
    ];
    if filter.contains(['*', '?']) {
        fields.iter().any(|field| glob_match(filter, field))
    } else {
        let needle = filter.to_lowercase();
        fields.iter().any(|field| field.to_lowercase().contains(&needle))
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
