use std::path::Path;

use super::*;
use crate::app::testing::{entry, test_app};

#[test]
fn copying_without_a_session_warns() {
    let mut test = test_app(Path::new("/d"));
    test.list_local(vec![entry(Path::new("/d"), "a.txt", false)]);
    test.app.local.cursor = 1;

    test.app.apply_action(Action::Copy);

    assert_eq!(test.notification().as_deref(), Some("Connect to a remote server first"));
    assert!(test.sent().is_empty());
}

#[test]
fn copying_from_the_local_panel_uploads_into_the_remote_folder() {
    let mut test = test_app(Path::new("/d"));
    let session = test.connect(3, "srv");
    test.list_remote(session, "/srv/in", Vec::new());
    test.list_local(vec![entry(Path::new("/d"), "a.txt", false)]);
    test.app.local.cursor = 1;

    test.app.apply_action(Action::Copy);

    assert_eq!(
        test.sent(),
        vec![Command::Copy {
            from: Location::Local,
            entries: vec![entry(Path::new("/d"), "a.txt", false)],
            to: Location::Session(session),
            dest_dir: PathBuf::from("/srv/in"),
        }]
    );
}

#[test]
fn copying_from_the_remote_panel_downloads_into_the_local_folder() {
    let mut test = test_app(Path::new("/d"));
    let session = test.connect(3, "srv");
    test.list_remote(session, "/srv", vec![entry(Path::new("/srv"), "b.bin", false)]);
    test.app.active_panel = ActivePanel::Remote;
    test.app.sessions.active_mut().unwrap().panel.cursor = 1;

    test.app.apply_action(Action::Copy);

    assert_eq!(
        test.sent(),
        vec![Command::Copy {
            from: Location::Session(session),
            entries: vec![entry(Path::new("/srv"), "b.bin", false)],
            to: Location::Local,
            dest_dir: PathBuf::from("/d"),
        }]
    );
}

#[test]
fn copying_with_nothing_under_the_cursor_sends_nothing() {
    let mut test = test_app(Path::new("/d"));
    test.connect(3, "srv");

    test.app.apply_action(Action::Copy);

    assert!(test.sent().is_empty());
}

#[test]
fn cancel_all_copies_asks_the_core() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_action(Action::CancelTransfer);

    assert_eq!(test.sent(), vec![Command::CancelAllTransfers]);
}

#[test]
fn copying_is_only_available_on_the_files_screen() {
    let mut test = test_app(Path::new("/d"));
    test.connect(3, "srv");
    test.list_local(vec![entry(Path::new("/d"), "a.txt", false)]);
    test.app.local.cursor = 1;
    test.app.screen = Screen::Connections;

    test.app.apply_action(Action::Copy);

    assert!(test.sent().is_empty());
}
