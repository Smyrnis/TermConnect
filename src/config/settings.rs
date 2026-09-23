use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSettings {
    pub show_hidden: bool,
    pub sort_key: String,
    pub sort_order: String,
}

impl Default for PanelSettings {
    fn default() -> Self {
        Self { show_hidden: false, sort_key: "name".to_string(), sort_order: "ascending".to_string() }
    }
}

pub const MAX_PARALLEL_CAP: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSettings {
    pub max_parallel: usize,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self { max_parallel: 4 }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub panel: PanelSettings,
    pub keys: HashMap<String, String>,
    pub transfers: TransferSettings,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct SettingsFile {
    #[serde(default)]
    pub panel: PanelSettingsFile,
    #[serde(default)]
    pub keys: HashMap<String, String>,
    #[serde(default)]
    pub transfers: TransferSettingsFile,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct TransferSettingsFile {
    pub max_parallel: Option<i64>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct PanelSettingsFile {
    pub show_hidden: Option<bool>,
    pub sort_key: Option<String>,
    pub sort_order: Option<String>,
}

pub(crate) fn settings_from_file(file: SettingsFile) -> (Settings, Vec<String>) {
    let mut warnings = Vec::new();
    let defaults = PanelSettings::default();

    let show_hidden = file.panel.show_hidden.unwrap_or(defaults.show_hidden);

    let sort_key = match file.panel.sort_key {
        Some(value) if value == "name" || value == "size" => value,
        Some(other) => {
            warnings.push(format!("unknown panel.sort_key \"{other}\", using \"name\""));
            defaults.sort_key.clone()
        }
        None => defaults.sort_key.clone(),
    };

    let sort_order = match file.panel.sort_order {
        Some(value) if value == "ascending" || value == "descending" => value,
        Some(other) => {
            warnings.push(format!("unknown panel.sort_order \"{other}\", using \"ascending\""));
            defaults.sort_order.clone()
        }
        None => defaults.sort_order.clone(),
    };

    let max_parallel = match file.transfers.max_parallel {
        Some(value) if value > MAX_PARALLEL_CAP as i64 => {
            warnings.push(format!("transfers.max_parallel is capped at {MAX_PARALLEL_CAP}, using {MAX_PARALLEL_CAP}"));
            MAX_PARALLEL_CAP
        }
        Some(value) if value >= 1 => value as usize,
        Some(_) => {
            warnings.push("transfers.max_parallel must be at least 1, using 1".to_string());
            1
        }
        None => TransferSettings::default().max_parallel,
    };

    (Settings { panel: PanelSettings { show_hidden, sort_key, sort_order }, keys: file.keys, transfers: TransferSettings { max_parallel } }, warnings)
}

#[cfg(test)]
#[path = "../../tests/config/settings_test.rs"]
mod tests;
