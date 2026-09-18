use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Where log output is written — kept out of the terminal entirely, since
/// `tracing_subscriber`'s default stdout writer would print over the TUI's
/// alternate screen the moment `RUST_LOG` is set.
pub fn log_path() -> Result<PathBuf> {
    Ok(state_dir()?.join("termconnect.log"))
}

fn state_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        return Ok(PathBuf::from(xdg).join("termconnect"));
    }
    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join(".local").join("state").join("termconnect"))
}

/// Opens the log file for appending, creating its parent directory (and
/// the file itself) if missing. Appends rather than truncates so a
/// session's log doesn't erase the previous run's.
pub fn open_writer() -> Result<File> {
    open_writer_at(&log_path()?)
}

fn open_writer_at(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open log file at {}", path.display()))
}

#[cfg(test)]
#[path = "../tests/root/logging_test.rs"]
mod tests;
