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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub panel: PanelSettings,
    pub keys: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct SettingsFile {
    #[serde(default)]
    pub panel: PanelSettingsFile,
    #[serde(default)]
    pub keys: HashMap<String, String>,
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

    (Settings { panel: PanelSettings { show_hidden, sort_key, sort_order }, keys: file.keys }, warnings)
}

#[cfg(test)]
#[path = "../../tests/config/settings_test.rs"]
mod tests;
