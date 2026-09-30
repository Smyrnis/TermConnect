use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

use anyhow::{Context, Result};
use tracing::{Level, Subscriber};
use tracing_subscriber::{
    EnvFilter,
    filter::{FilterExt, Targets},
    layer::Filter,
    registry::LookupSpan,
};

const TRANSFER_TARGET: &str = "porthmos::transfers";

pub fn with_transfer_log<S>(filter: EnvFilter) -> impl Filter<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    filter.or(Targets::new().with_target(TRANSFER_TARGET, Level::ERROR))
}

pub fn open_writer_at(path: &Path) -> Result<File> {
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
mod tests;
