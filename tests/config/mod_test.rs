use std::fs;

use super::*;

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
