use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::Paths;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    pub save_passwords_in_keyring: bool,
}

pub fn load(paths: &Paths) -> UiState {
    fs::read_to_string(paths.state_file()).ok().and_then(|contents| toml::from_str(&contents).ok()).unwrap_or_default()
}

pub fn save(paths: &Paths, state: &UiState) -> Result<()> {
    let path = paths.state_file();
    fs::create_dir_all(&paths.state_dir)?;
    let temp_path = {
        let mut name = path.as_os_str().to_owned();
        name.push(".tmp");
        PathBuf::from(name)
    };
    let mut handle = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&temp_path)?;
    handle.write_all(toml::to_string(state)?.as_bytes())?;
    drop(handle);
    fs::rename(&temp_path, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
