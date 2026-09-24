use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::profile::ConnectionProfile;
use crate::Paths;

#[derive(Debug, Default, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(default)]
    connections: BTreeMap<String, ConnectionProfile>,
}

pub fn load(paths: &Paths) -> Result<Vec<ConnectionProfile>> {
    load_from(&paths.connections_file())
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

pub fn save(paths: &Paths, profile: &ConnectionProfile) -> Result<()> {
    save_to(&paths.connections_file(), profile)
}

fn save_to(path: &Path, profile: &ConnectionProfile) -> Result<()> {
    let mut file = read_config_file(path)?;
    file.connections.insert(profile.name.clone(), profile.clone());
    write_config_file(path, &file)
}

pub fn delete(paths: &Paths, name: &str) -> Result<()> {
    delete_from(&paths.connections_file(), name)
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

fn write_config_file(path: &Path, file: &ConfigFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = {
        let mut name = path.as_os_str().to_owned();
        name.push(".tmp");
        PathBuf::from(name)
    };
    let mut handle = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temp_path)?;
    handle.write_all(toml::to_string_pretty(file)?.as_bytes())?;
    drop(handle);
    fs::rename(&temp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
