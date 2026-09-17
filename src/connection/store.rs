use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::profile::ConnectionProfile;

#[derive(Debug, Default, Deserialize, Serialize)]
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

fn load_from(path: &Path) -> Result<Vec<ConnectionProfile>> {
    Ok(read_config_file(path)?
        .connections
        .into_iter()
        .map(|(name, mut profile)| {
            profile.name = name;
            profile
        })
        .collect())
}

/// Saves `profile`, inserting it or overwriting an existing profile with the
/// same name.
pub fn save(profile: &ConnectionProfile) -> Result<()> {
    save_to(&config_path()?, profile)
}

fn save_to(path: &Path, profile: &ConnectionProfile) -> Result<()> {
    let mut file = read_config_file(path)?;
    file.connections
        .insert(profile.name.clone(), profile.clone());
    write_config_file(path, &file)
}

/// Removes the profile named `name`, if one exists.
pub fn delete(name: &str) -> Result<()> {
    delete_from(&config_path()?, name)
}

fn delete_from(path: &Path, name: &str) -> Result<()> {
    let mut file = read_config_file(path)?;
    file.connections.remove(name);
    write_config_file(path, &file)
}

fn read_config_file(path: &Path) -> Result<ConfigFile> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(toml::from_str(&contents)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(err) => Err(err.into()),
    }
}

/// Rewrites the whole file — small and human-editable, so there's no need
/// for incremental writes — then restricts it to owner read/write only,
/// since a profile may now carry a plaintext password.
fn write_config_file(path: &Path, file: &ConfigFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Open (or create) with 0600 from the very first `open(2)` call, so a
    // freshly-created file — which may carry a plaintext password — is
    // never briefly readable at the process umask (e.g. 0644) before the
    // permissions get locked down below.
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    handle.write_all(toml::to_string_pretty(file)?.as_bytes())?;
    // Still needed for a file that already existed at looser permissions
    // (e.g. from before this fix, or manual editing) — `.mode(0o600)` above
    // only governs the permissions used at creation time.
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/connection/store_test.rs"]
mod tests;
