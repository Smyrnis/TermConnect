use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config;

use super::profile::ConnectionProfile;

#[derive(Debug, Default, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(default)]
    connections: BTreeMap<String, ConnectionProfile>,
}

/// Connections live in their own file, separate from `config.toml`'s
/// `[panel]`/`[keys]` settings — `write_config_file` below rewrites this
/// file wholesale on every save/delete, which would silently drop those
/// sections if they shared a file. Resolved via `config::config_dir` so
/// profiles and settings always land under the same directory.
pub fn config_path() -> Result<PathBuf> {
    Ok(config::config_dir()?.join("connections.toml"))
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
    file.connections.insert(profile.name.clone(), profile.clone());
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

/// Writes the whole file atomically — small and human-editable, so
/// there's no need for incremental writes — by writing to a temp file in
/// the same directory and renaming it into place, so a crash or full disk
/// mid-write leaves the previous, still-intact file at `path` rather than
/// a truncated or empty one. Restricted to owner read/write only, since a
/// profile may now carry a plaintext password.
fn write_config_file(path: &Path, file: &ConfigFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = {
        let mut name = path.as_os_str().to_owned();
        name.push(".tmp");
        PathBuf::from(name)
    };
    // Open (or create) with 0600 from the very first `open(2)` call, so
    // the temp file — which may carry a plaintext password — is never
    // briefly readable at the process umask (e.g. 0644) before the
    // permissions get locked down. `rename` below carries this file's
    // permissions to the final path, so no separate `set_permissions`
    // step is needed afterward.
    let mut handle = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temp_path)?;
    handle.write_all(toml::to_string_pretty(file)?.as_bytes())?;
    drop(handle);
    fs::rename(&temp_path, path)?;
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/connection/store_test.rs"]
mod tests;
