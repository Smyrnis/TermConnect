use std::fs;

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

#[test]
fn mkdir_dialog_creates_a_directory_on_submit() {
    let (dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::Mkdir);
    assert!(app.dialog.is_some());

    for c in "new_dir".chars() {
        app.apply_dialog_key(key(KeyCode::Char(c)));
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    assert!(app.dialog.is_none());
    assert!(dir.path().join("new_dir").is_dir());
}

#[test]
fn mkdir_dialog_cancelled_with_esc_creates_nothing() {
    let (dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::Mkdir);
    app.apply_dialog_key(key(KeyCode::Char('x')));
    app.apply_dialog_key(key(KeyCode::Esc));

    assert!(app.dialog.is_none());
    assert!(!dir.path().join("x").exists());
}

#[test]
fn delete_dialog_confirmed_with_y_deletes_the_target() {
    let (dir, mut app) = app_in_temp_dir();
    fs::write(dir.path().join("doomed.txt"), b"content").unwrap();
    app.local.refresh().unwrap();
    app.local.cursor = app.local.rows().len() - 1;

    app.apply_action(Action::Delete);
    assert!(app.dialog.is_some());
    app.apply_dialog_key(key(KeyCode::Char('y')));

    assert!(app.dialog.is_none());
    assert!(!dir.path().join("doomed.txt").exists());
}

#[test]
fn delete_dialog_cancelled_with_n_deletes_nothing() {
    let (dir, mut app) = app_in_temp_dir();
    fs::write(dir.path().join("safe.txt"), b"content").unwrap();
    app.local.refresh().unwrap();
    app.local.cursor = app.local.rows().len() - 1;

    app.apply_action(Action::Delete);
    app.apply_dialog_key(key(KeyCode::Char('n')));

    assert!(app.dialog.is_none());
    assert!(dir.path().join("safe.txt").exists());
}

#[test]
fn rename_dialog_prefills_the_current_name() {
    let (dir, mut app) = app_in_temp_dir();
    fs::write(dir.path().join("old.txt"), b"content").unwrap();
    app.local.refresh().unwrap();
    app.local.cursor = app.local.rows().len() - 1;

    app.apply_action(Action::Rename);

    match app.dialog {
        Some(Dialog::TextInput(ref dialog)) => assert_eq!(dialog.value, "old.txt"),
        _ => panic!("expected a text input dialog"),
    }
}

#[test]
fn failed_operation_sets_a_status_message() {
    let (_dir, mut app) = app_in_temp_dir();
    app.local.create_directory("dup").unwrap();
    app.apply_action(Action::Mkdir);
    for c in "dup".chars() {
        app.apply_dialog_key(key(KeyCode::Char(c)));
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    assert!(app.notifications.current().is_some());
}
