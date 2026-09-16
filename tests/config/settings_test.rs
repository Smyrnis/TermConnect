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
    let file = SettingsFile {
        panel: PanelSettingsFile::default(),
        keys: keys.clone(),
    };

    let (settings, _) = settings_from_file(file);

    assert_eq!(settings.keys, keys);
}
