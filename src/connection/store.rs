use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::profile::ConnectionProfile;

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    connections: BTreeMap<String, ConnectionProfile>,
}

pub fn config_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("termconnect")
        .join("config.toml"))
}

/// Loads saved connection profiles. A missing config file is not an error —
/// it simply means no profiles have been saved yet.
pub fn load() -> Result<Vec<ConnectionProfile>> {
    load_from(&config_path()?)
}

fn load_from(path: &std::path::Path) -> Result<Vec<ConnectionProfile>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    let file: ConfigFile = toml::from_str(&contents)?;

    Ok(file
        .connections
        .into_iter()
        .map(|(name, mut profile)| {
            profile.name = name;
            profile
        })
        .collect())
}

#[cfg(test)]
#[path = "../../tests/connection/store_test.rs"]
mod tests;
