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
mod tests {
    use super::*;

    #[test]
    fn load_from_a_missing_file_returns_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let profiles = load_from(&path).unwrap();

        assert!(profiles.is_empty());
    }

    #[test]
    fn load_from_parses_connection_tables_and_fills_in_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[connections.production]
host = "server.example.com"
port = 2222
username = "deploy"
identity_file = "/home/user/.ssh/id_ed25519"
remote_path = "/var/www/app"
"#,
        )
        .unwrap();

        let profiles = load_from(&path).unwrap();

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "production");
        assert_eq!(profiles[0].host, "server.example.com");
        assert_eq!(profiles[0].port, 2222);
        assert_eq!(profiles[0].username, "deploy");
        assert_eq!(
            profiles[0].identity_file,
            Some(PathBuf::from("/home/user/.ssh/id_ed25519"))
        );
    }

    #[test]
    fn load_from_defaults_port_to_22_when_omitted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[connections.staging]
host = "staging.example.com"
username = "deploy"
"#,
        )
        .unwrap();

        let profiles = load_from(&path).unwrap();

        assert_eq!(profiles[0].port, 22);
    }
}
