use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use porthmos_vfs::Environment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigMigration {
    NothingToMigrate,
    Moved { from: PathBuf, to: PathBuf },
    KeptBoth { legacy: PathBuf },
    Failed { from: PathBuf, error: String },
}

const APP_DIR: &str = "porthmos";
const LEGACY_APP_DIR: &str = "termconnect";
const MISSING_HOME: &str = "HOME environment variable is not set";

fn resolve_dir(xdg: Option<OsString>, home: Option<&Path>, fallback: &[&str]) -> Result<PathBuf> {
    if let Some(xdg) = xdg {
        return Ok(PathBuf::from(xdg).join(APP_DIR));
    }
    let home = home.context(MISSING_HOME)?;
    Ok(fallback.iter().fold(home.to_path_buf(), |path, part| path.join(part)).join(APP_DIR))
}

impl Paths {
    pub fn from_env(env: &Environment) -> Result<Self> {
        Self::resolve(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("XDG_STATE_HOME"), env.home.as_deref())
    }

    pub fn resolve(xdg_config: Option<OsString>, xdg_state: Option<OsString>, home: Option<&Path>) -> Result<Self> {
        Ok(Self {
            config_dir: resolve_dir(xdg_config, home, &[".config"])?,
            state_dir: resolve_dir(xdg_state, home, &[".local", "state"])?,
        })
    }

    pub fn in_dir(root: &Path) -> Self {
        Self { config_dir: root.join("config"), state_dir: root.join("state") }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn connections_file(&self) -> PathBuf {
        self.config_dir.join("connections.toml")
    }

    pub fn known_certificates_file(&self) -> PathBuf {
        self.config_dir.join("known_certificates.toml")
    }

    pub fn bookmarks_file(&self) -> PathBuf {
        self.config_dir.join("bookmarks.toml")
    }

    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("porthmos.log")
    }

    pub fn migrate_legacy_config(&self) -> ConfigMigration {
        let Some(legacy) = self.config_dir.parent().map(|parent| parent.join(LEGACY_APP_DIR)) else {
            return ConfigMigration::NothingToMigrate;
        };
        if !legacy.is_dir() {
            return ConfigMigration::NothingToMigrate;
        }
        if self.config_dir.exists() {
            tracing::info!(legacy = %legacy.display(), "both the old and the new config folder exist; using the new one");
            return ConfigMigration::KeptBoth { legacy };
        }
        match std::fs::rename(&legacy, &self.config_dir) {
            Ok(()) => {
                tracing::info!(from = %legacy.display(), to = %self.config_dir.display(), "moved the old config folder");
                ConfigMigration::Moved { from: legacy, to: self.config_dir.clone() }
            }
            Err(err) => {
                tracing::warn!(from = %legacy.display(), to = %self.config_dir.display(), "could not move the old config folder, starting with an empty config: {err}");
                ConfigMigration::Failed { from: legacy, error: err.to_string() }
            }
        }
    }
}

#[cfg(test)]
mod tests;
