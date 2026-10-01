use std::{collections::BTreeMap, fs, path::Path};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{labels::Labels, profile::ConnectionProfile};
use crate::Paths;

#[derive(Debug, Default, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(default)]
    connections: BTreeMap<String, ConnectionProfile>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    ssh_hosts: BTreeMap<String, Labels>,
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

pub fn load_ssh_labels(paths: &Paths) -> Result<BTreeMap<String, Labels>> {
    Ok(read_config_file(&paths.connections_file())?.ssh_hosts)
}

pub fn save_ssh_labels(paths: &Paths, name: &str, labels: &Labels) -> Result<()> {
    let path = paths.connections_file();
    let mut file = read_config_file(&path)?;
    let in_keyring = file.ssh_hosts.get(name).map(|record| record.in_keyring.clone()).unwrap_or_default();
    let record = Labels { group: labels.group.clone(), tags: labels.tags.clone(), in_keyring };
    if record.is_empty() {
        file.ssh_hosts.remove(name);
    } else {
        file.ssh_hosts.insert(name.to_string(), record);
    }
    write_config_file(&path, &file)
}

pub fn set_profile_markers(paths: &Paths, name: &str, markers: &[String]) -> Result<bool> {
    let path = paths.connections_file();
    let mut file = read_config_file(&path)?;
    let Some(profile) = file.connections.get_mut(name) else {
        return Ok(false);
    };
    profile.in_keyring = markers.to_vec();
    write_config_file(&path, &file)?;
    Ok(true)
}

pub fn set_ssh_markers(paths: &Paths, alias: &str, markers: &[String]) -> Result<()> {
    let path = paths.connections_file();
    let mut file = read_config_file(&path)?;
    let mut record = file.ssh_hosts.remove(alias).unwrap_or_default();
    record.in_keyring = markers.to_vec();
    if !record.is_empty() {
        file.ssh_hosts.insert(alias.to_string(), record);
    }
    write_config_file(&path, &file)
}

pub fn move_ssh_labels(paths: &Paths, from: &str, to: &str) -> Result<()> {
    let path = paths.connections_file();
    let mut file = read_config_file(&path)?;
    if let Some(labels) = file.ssh_hosts.remove(from) {
        file.ssh_hosts.insert(to.to_string(), labels);
    }
    write_config_file(&path, &file)
}

pub fn forget_ssh_labels(paths: &Paths, name: &str) -> Result<()> {
    let path = paths.connections_file();
    let mut file = read_config_file(&path)?;
    file.ssh_hosts.remove(name);
    write_config_file(&path, &file)
}

fn read_config_file(path: &Path) -> Result<ConfigFile> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(toml::from_str(&contents)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(err) => Err(err.into()),
    }
}

fn write_config_file(path: &Path, file: &ConfigFile) -> Result<()> {
    crate::persist::write_atomic(path, toml::to_string_pretty(file)?.as_bytes(), 0o600)
}

#[cfg(test)]
mod tests;
