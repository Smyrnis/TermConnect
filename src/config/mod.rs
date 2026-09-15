pub mod settings;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub use settings::Settings;

/// A non-fatal problem found while loading configuration — surfaced as a
/// `Warning`-severity notification at startup (Task 12) rather than
/// blocking the app from running.
pub struct StartupWarning(pub String);

pub fn config_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("termconnect"));
    }
    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join(".config").join("termconnect"))
}

/// Loads `config.toml`. A missing file is not an error — it just means no
/// settings have been saved yet. A file that fails to parse, or that
/// contains an unrecognized `[panel]` value, recovers to defaults for the
/// broken part and reports one warning each; loading never blocks startup.
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
            return Ok((
                Settings::default(),
                vec![StartupWarning(format!(
                    "failed to parse config.toml: {err}"
                ))],
            ));
        }
    };

    let (settings, warnings) = settings::settings_from_file(file);
    Ok((settings, warnings.into_iter().map(StartupWarning).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_from_a_missing_file_returns_defaults_and_no_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let (settings, warnings) = load_from(&path).unwrap();

        assert_eq!(settings, Settings::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn load_from_parses_a_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[panel]
show_hidden = true
sort_key = "size"

[keys]
quit = "ctrl+q"
"#,
        )
        .unwrap();

        let (settings, warnings) = load_from(&path).unwrap();

        assert!(warnings.is_empty());
        assert!(settings.panel.show_hidden);
        assert_eq!(settings.panel.sort_key, "size");
        assert_eq!(settings.keys.get("quit"), Some(&"ctrl+q".to_string()));
    }

    #[test]
    fn load_from_recovers_to_defaults_on_malformed_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "this is not [ valid toml").unwrap();

        let (settings, warnings) = load_from(&path).unwrap();

        assert_eq!(settings, Settings::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].0.contains("config.toml"));
    }
}
