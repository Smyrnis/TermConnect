use std::path::Path;

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::*;
use crate::app::testing::{TestApp, entry, test_app};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn app_with_file(name: &str) -> TestApp {
    let mut test = test_app(Path::new("/d"));
    test.list_local(vec![entry(Path::new("/d"), name, false)]);
    test.app.local.cursor = test.app.local.rows().len() - 1;
    test
}

#[test]
fn mkdir_dialog_creates_a_directory_on_submit() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::Mkdir);
    assert!(test.app.dialog.is_some());

    for c in "new_dir".chars() {
        test.app.apply_dialog_key(key(KeyCode::Char(c)));
    }
    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert!(test.app.dialog.is_none());
    assert_eq!(test.sent(), vec![Command::CreateDir { location: Location::Local, path: PathBuf::from("/d/new_dir") }]);
}

#[test]
fn mkdir_on_the_remote_panel_goes_to_that_session() {
    let mut test = test_app(Path::new("/d"));
    let session = test.connect(2, "srv");
    test.list_remote(session, "/srv", Vec::new());
    test.app.active_panel = ActivePanel::Remote;
    test.app.apply_action(Action::Mkdir);

    test.app.apply_dialog_key(key(KeyCode::Char('x')));
    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::CreateDir { location: Location::Session(session), path: PathBuf::from("/srv/x") }]
    );
}

#[test]
fn mkdir_dialog_cancelled_with_esc_creates_nothing() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::Mkdir);
    test.app.apply_dialog_key(key(KeyCode::Char('x')));
    test.app.apply_dialog_key(key(KeyCode::Esc));

    assert!(test.app.dialog.is_none());
    assert!(test.sent().is_empty());
}

#[test]
fn delete_dialog_confirmed_with_y_deletes_the_target() {
    let mut test = app_with_file("doomed.txt");

    test.app.apply_action(Action::Delete);
    assert!(test.app.dialog.is_some());
    test.app.apply_dialog_key(key(KeyCode::Char('y')));

    assert!(test.app.dialog.is_none());
    assert_eq!(
        test.sent(),
        vec![Command::Delete { location: Location::Local, paths: vec![PathBuf::from("/d/doomed.txt")] }]
    );
}

#[test]
fn delete_dialog_cancelled_with_n_deletes_nothing() {
    let mut test = app_with_file("safe.txt");

    test.app.apply_action(Action::Delete);
    test.app.apply_dialog_key(key(KeyCode::Char('n')));

    assert!(test.app.dialog.is_none());
    assert!(test.sent().is_empty());
}

#[test]
fn rename_dialog_prefills_the_current_name() {
    let mut test = app_with_file("old.txt");

    test.app.apply_action(Action::Rename);

    match test.app.dialog {
        Some(Dialog::TextInput(ref dialog)) => assert_eq!(dialog.value, "old.txt"),
        _ => panic!("expected a text input dialog"),
    }
}

#[test]
fn rename_dialog_submits_the_rename() {
    let mut test = app_with_file("old.txt");
    test.app.apply_action(Action::Rename);
    if let Some(Dialog::TextInput(dialog)) = test.app.dialog.as_mut() {
        dialog.value = "new.txt".to_string();
    }

    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::Rename {
            location: Location::Local,
            from: PathBuf::from("/d/old.txt"),
            to: PathBuf::from("/d/new.txt"),
        }]
    );
}

#[test]
fn failed_operation_sets_a_status_message() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_core_event(Event::Notice {
        severity: Severity::Error,
        message: "File exists (os error 17)".to_string(),
    });

    assert!(test.app.notifications.current().is_some());
}
