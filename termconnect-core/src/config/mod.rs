pub mod bookmarks;
pub mod settings;

use std::{fs, path::Path};

use anyhow::Result;
pub use settings::Settings;

use crate::Paths;

pub struct StartupWarning(pub String);

pub fn load(paths: &Paths) -> Result<(Settings, Vec<StartupWarning>)> {
    load_from(&paths.config_file())
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
mod tests;
