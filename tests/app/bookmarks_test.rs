use super::*;
use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
use std::fs;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

#[test]
fn bookmark_here_action_opens_a_text_input_dialog_prefilled_with_the_directory_name() {
    let (dir, mut app) = app_in_temp_dir();

    app.apply_action(Action::BookmarkHere);

    match app.dialog {
        Some(Dialog::TextInput(ref d)) => {
            assert_eq!(d.value, dir.path().file_name().unwrap().to_str().unwrap());
        }
        _ => panic!("expected a text input dialog"),
    }
}

#[test]
fn submitting_the_bookmark_dialog_adds_a_local_bookmark() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::BookmarkHere);

    app.apply_dialog_key(key(KeyCode::Char('x')));
    app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(app.bookmarks.len(), 1);
    assert_eq!(app.bookmarks.get(0).unwrap().host, None);
}

#[test]
fn open_bookmarks_action_lists_saved_bookmarks() {
    let (dir, mut app) = app_in_temp_dir();
    app.bookmarks.add(config::bookmarks::Bookmark {
        label: "here".to_string(),
        path: dir.path().to_path_buf(),
        host: None,
    });

    app.apply_action(Action::OpenBookmarks);

    match app.dialog {
        Some(Dialog::List(ref d)) => assert_eq!(d.items.len(), 1),
        _ => panic!("expected a list dialog"),
    }
}

#[test]
fn selecting_a_local_bookmark_navigates_the_local_panel() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    let mut app = App::at(dir.path().to_path_buf()).unwrap();
    app.bookmarks.add(config::bookmarks::Bookmark { label: "child".to_string(), path: child.clone(), host: None });
    app.apply_action(Action::OpenBookmarks);

    app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(app.local.path(), child);
}

#[test]
fn selecting_a_remote_bookmark_without_a_connection_warns_instead_of_navigating() {
    let (_dir, mut app) = app_in_temp_dir();
    app.bookmarks.add(config::bookmarks::Bookmark {
        label: "prod etc".to_string(),
        path: PathBuf::from("/etc"),
        host: Some("production".to_string()),
    });
    app.apply_action(Action::OpenBookmarks);

    app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(app.notifications.current().unwrap().message, "Connect to production first");
}

#[test]
fn removing_a_bookmark_deletes_it_from_the_list() {
    let (_dir, mut app) = app_in_temp_dir();
    app.bookmarks.add(config::bookmarks::Bookmark { label: "a".to_string(), path: PathBuf::from("/a"), host: None });
    app.apply_action(Action::OpenBookmarks);

    app.apply_dialog_key(key(KeyCode::F(8)));

    assert!(app.bookmarks.is_empty());
}
