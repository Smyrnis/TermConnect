use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

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
