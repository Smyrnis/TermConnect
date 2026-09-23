pub mod bookmarks;
pub mod settings;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub use settings::Settings;

pub struct StartupWarning(pub String);

pub fn config_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("termconnect"));
    }
    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join(".config").join("termconnect"))
}

pub fn load() -> Result<(Settings, Vec<StartupWarning>)> {
    load_from(&config_dir()?.join("config.toml"))
}

fn load_from(path: &Path) -> Result<(Settings, Vec<StartupWarning>)> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Settings::default(), Vec::new()));
        }
        Err(err) => return Err(err.into()),
    };

    let file: settings::SettingsFile = match toml::from_str(&contents) {
        Ok(file) => file,
        Err(err) => {
            return Ok((Settings::default(), vec![StartupWarning(format!("failed to parse config.toml: {err}"))]));
        }
    };

    let (settings, warnings) = settings::settings_from_file(file);
    Ok((settings, warnings.into_iter().map(StartupWarning).collect()))
}

#[cfg(test)]
#[path = "../../tests/config/mod_test.rs"]
mod tests;
