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
        Self {
            show_hidden: false,
            sort_key: "name".to_string(),
            sort_order: "ascending".to_string(),
        }
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

/// Converts a parsed TOML file into validated `Settings`. An unrecognized
/// `[panel]` value falls back to the default and produces one warning;
/// `[keys]` passes through untouched (see the module-level interface note
/// on why validation happens in `App` instead).
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
            warnings.push(format!(
                "unknown panel.sort_order \"{other}\", using \"ascending\""
            ));
            defaults.sort_order.clone()
        }
        None => defaults.sort_order.clone(),
    };

    (
        Settings { panel: PanelSettings { show_hidden, sort_key, sort_order }, keys: file.keys },
        warnings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_from_file_uses_defaults_when_panel_is_absent() {
        let (settings, warnings) = settings_from_file(SettingsFile::default());

        assert!(warnings.is_empty());
        assert!(!settings.panel.show_hidden);
        assert_eq!(settings.panel.sort_key, "name");
        assert_eq!(settings.panel.sort_order, "ascending");
    }

    #[test]
    fn settings_from_file_accepts_valid_panel_values() {
        let file = SettingsFile {
            panel: PanelSettingsFile {
                show_hidden: Some(true),
                sort_key: Some("size".to_string()),
                sort_order: Some("descending".to_string()),
            },
            keys: HashMap::new(),
        };

        let (settings, warnings) = settings_from_file(file);

        assert!(warnings.is_empty());
        assert!(settings.panel.show_hidden);
        assert_eq!(settings.panel.sort_key, "size");
        assert_eq!(settings.panel.sort_order, "descending");
    }

    #[test]
    fn settings_from_file_falls_back_and_warns_on_an_unknown_sort_key() {
        let file = SettingsFile {
            panel: PanelSettingsFile {
                show_hidden: None,
                sort_key: Some("date".to_string()),
                sort_order: None,
            },
            keys: HashMap::new(),
        };

        let (settings, warnings) = settings_from_file(file);

        assert_eq!(settings.panel.sort_key, "name");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("date"));
    }

    #[test]
    fn settings_from_file_passes_keys_through_unvalidated() {
        let mut keys = HashMap::new();
        keys.insert("quit".to_string(), "ctrl+q".to_string());
        let file = SettingsFile { panel: PanelSettingsFile::default(), keys: keys.clone() };

        let (settings, _) = settings_from_file(file);

        assert_eq!(settings.keys, keys);
    }
}
