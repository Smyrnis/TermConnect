use std::path::Path;

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::*;
use crate::app::testing::{TestApp, test_app};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn app_with(bookmarks: Vec<Bookmark>) -> TestApp {
    let mut test = test_app(Path::new("/data/projects"));
    test.app.apply_core_event(Event::Bookmarks(bookmarks));
    test
}

fn bookmark(label: &str, path: &str, host: Option<&str>) -> Bookmark {
    Bookmark { label: label.to_string(), path: PathBuf::from(path), host: host.map(str::to_string) }
}

#[test]
fn bookmark_here_action_opens_a_text_input_dialog_prefilled_with_the_directory_name() {
    let mut test = app_with(Vec::new());

    test.app.apply_action(Action::BookmarkHere);

    match test.app.dialog {
        Some(Dialog::TextInput(ref d)) => assert_eq!(d.value, "projects"),
        _ => panic!("expected a text input dialog"),
    }
}

#[test]
fn submitting_the_bookmark_dialog_adds_a_local_bookmark() {
    let mut test = app_with(Vec::new());
    test.app.apply_action(Action::BookmarkHere);
    if let Some(Dialog::TextInput(dialog)) = test.app.dialog.as_mut() {
        dialog.value = "x".to_string();
    }

    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::AddBookmark {
            label: "x".to_string(),
            location: Location::Local,
            path: PathBuf::from("/data/projects"),
        }]
    );
}

#[test]
fn bookmarking_the_remote_panel_without_a_session_warns() {
    let mut test = app_with(Vec::new());
    test.app.active_panel = ActivePanel::Remote;

    test.app.add_bookmark("x".to_string());

    assert_eq!(test.notification().as_deref(), Some("Connect to a remote server first"));
    assert!(test.sent().is_empty());
}

#[test]
fn open_bookmarks_action_lists_saved_bookmarks() {
    let mut test = app_with(vec![bookmark("here", "/data", None)]);

    test.app.apply_action(Action::OpenBookmarks);

    match test.app.dialog {
        Some(Dialog::List(ref d)) => assert_eq!(d.items.len(), 1),
        _ => panic!("expected a list dialog"),
    }
}

#[test]
fn selecting_a_local_bookmark_navigates_the_local_panel() {
    let mut test = app_with(vec![bookmark("child", "/data/child", None)]);
    test.app.apply_action(Action::OpenBookmarks);

    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::List { location: Location::Local, path: Some(PathBuf::from("/data/child")) }]
    );
    assert_eq!(test.app.active_panel, ActivePanel::Local);
}

#[test]
fn selecting_a_remote_bookmark_switches_to_that_session() {
    let mut test = app_with(vec![bookmark("etc", "/etc", Some("production"))]);
    let production = test.connect(1, "production");
    test.connect(2, "staging");
    test.app.apply_action(Action::OpenBookmarks);

    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(test.app.sessions.active_id(), Some(production));
    assert_eq!(test.app.active_panel, ActivePanel::Remote);
    assert_eq!(
        test.sent(),
        vec![Command::List { location: Location::Session(production), path: Some(PathBuf::from("/etc")) }]
    );
}

#[test]
fn selecting_a_remote_bookmark_without_a_connection_warns_instead_of_navigating() {
    let mut test = app_with(vec![bookmark("prod etc", "/etc", Some("production"))]);
    test.app.apply_action(Action::OpenBookmarks);

    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(test.notification().as_deref(), Some("Connect to production first"));
}

#[test]
fn removing_a_bookmark_deletes_it_from_the_list() {
    let mut test = app_with(vec![bookmark("a", "/a", None)]);
    test.app.apply_action(Action::OpenBookmarks);

    test.app.apply_dialog_key(key(KeyCode::F(8)));

    assert_eq!(test.sent(), vec![Command::RemoveBookmark { index: 0 }]);
    assert!(matches!(test.app.dialog, Some(Dialog::List(ref list)) if list.items.is_empty()));
}
