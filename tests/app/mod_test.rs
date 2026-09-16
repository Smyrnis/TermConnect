use super::*;
use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
use std::fs;

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

#[test]
fn at_with_applies_panel_settings_to_the_local_panel() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".hidden"), b"x").unwrap();
    let settings = config::settings::PanelSettings {
        show_hidden: true,
        sort_key: "name".to_string(),
        sort_order: "ascending".to_string(),
    };

    let app = App::at_with(
        dir.path().to_path_buf(),
        &settings,
        input::KeyBindings::defaults(),
        config::bookmarks::Bookmarks::default(),
        None,
    )
    .unwrap();

    assert!(app.local.show_hidden());
}

#[test]
fn key_bindings_from_config_are_used_for_key_mapping() {
    let (_dir, mut app) = app_in_temp_dir();
    let mut overrides = std::collections::HashMap::new();
    overrides.insert("quit".to_string(), "ctrl+q".to_string());
    let (bindings, _) = input::KeyBindings::from_overrides(&overrides);
    app.key_bindings = bindings;

    let event = KeyEvent {
        code: KeyCode::Char('q'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };

    assert_eq!(app.key_bindings.map_key(event), Action::Quit);
}

#[test]
fn set_status_pushes_an_error_notification_on_failure() {
    let (_dir, mut app) = app_in_temp_dir();

    app.set_status(Err(anyhow::anyhow!("boom")));

    let current = app.notifications.current().unwrap();
    assert_eq!(current.message, "boom");
    assert_eq!(current.severity, Severity::Error);
}

#[test]
fn set_status_does_nothing_on_success() {
    let (_dir, mut app) = app_in_temp_dir();

    app.set_status(Ok(()));

    assert!(app.notifications.current().is_none());
}
