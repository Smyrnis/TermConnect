use std::path::PathBuf;

use super::*;
use crate::engine::{Command, testing::test_engine};

#[tokio::test]
async fn listing_the_local_side_sends_its_entries() {
    let mut t = test_engine();
    std::fs::write(t.dir.path().join("file.txt"), b"x").unwrap();
    let dir = t.dir.path().to_path_buf();

    t.engine.handle_command(Command::List { location: Location::Local, path: Some(dir.clone()) });

    match t.next_event().await {
        Event::Listed { location: Location::Local, path, entries } => {
            assert_eq!(path, dir);
            assert_eq!(entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), ["file.txt"]);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_local_listing_failure_shows_the_bare_error() {
    let mut t = test_engine();

    t.engine
        .handle_command(Command::List { location: Location::Local, path: Some(PathBuf::from("/definitely/missing")) });

    match t.next_event().await {
        Event::Notice { severity: Severity::Error, message } => {
            assert_eq!(message, "No such file or directory (os error 2)");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_remote_listing_failure_names_the_folder() {
    let mut t = test_engine();
    let (session, _) = t.add_session("srv");

    t.engine.handle_command(Command::List { location: Location::Session(session), path: Some(PathBuf::from("/gone")) });

    match t.next_event().await {
        Event::Notice { severity: Severity::Error, message } => assert!(message.starts_with("Unable to list /gone:\n")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_listing_without_a_path_uses_the_home_folder() {
    let mut t = test_engine();
    let (session, remote) = t.add_session("srv");
    remote.file("/home/user/a", b"", None);

    t.engine.handle_command(Command::List { location: Location::Session(session), path: None });

    assert!(matches!(t.next_event().await, Event::Listed { path, .. } if path == std::path::Path::new("/home/user")));
}

#[test]
fn operations_on_a_vanished_session_are_dropped() {
    let mut t = test_engine();

    t.engine.handle_command(Command::List { location: Location::Session(42), path: None });
    t.engine.handle_command(Command::CreateDir { location: Location::Session(42), path: PathBuf::from("/x") });

    assert!(t.drain().is_empty());
}

#[tokio::test]
async fn mkdir_creates_the_directory_and_reports_the_change() {
    let mut t = test_engine();
    let target = t.dir.path().join("new_dir");

    t.engine.handle_command(Command::CreateDir { location: Location::Local, path: target.clone() });

    assert!(matches!(t.next_event().await, Event::LocationChanged { location: Location::Local }));
    assert!(target.is_dir());
}

#[tokio::test]
async fn a_failed_mkdir_shows_an_error_and_does_not_report_a_change() {
    let mut t = test_engine();
    let target = t.dir.path().join("dup");
    std::fs::create_dir(&target).unwrap();

    t.engine.handle_command(Command::CreateDir { location: Location::Local, path: target });

    assert!(matches!(t.next_event().await, Event::Notice { severity: Severity::Error, .. }));
    tokio::task::yield_now().await;
    assert!(t.drain().is_empty());
}

#[tokio::test]
async fn a_failed_remote_mkdir_says_what_failed() {
    let mut t = test_engine();
    let (session, _) = t.add_session("srv");

    t.engine.handle_command(Command::CreateDir {
        location: Location::Session(session),
        path: PathBuf::from("/no/parent/x"),
    });

    match t.next_event().await {
        Event::Notice { message, .. } => assert!(message.starts_with("Unable to create directory:\n")),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn rename_moves_the_entry() {
    let mut t = test_engine();
    std::fs::write(t.dir.path().join("old.txt"), b"x").unwrap();

    t.engine.handle_command(Command::Rename {
        location: Location::Local,
        from: t.dir.path().join("old.txt"),
        to: t.dir.path().join("new.txt"),
    });

    assert!(matches!(t.next_event().await, Event::LocationChanged { location: Location::Local }));
    assert!(t.dir.path().join("new.txt").exists());
    assert!(!t.dir.path().join("old.txt").exists());
}

#[tokio::test]
async fn deleting_a_local_folder_emits_location_changed() {
    let mut t = test_engine();
    let target = t.dir.path().join("old");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("inside"), b"x").unwrap();

    t.engine.handle_command(Command::Delete { location: Location::Local, paths: vec![target.clone()] });

    assert!(matches!(t.next_event().await, Event::LocationChanged { location: Location::Local }));
    assert!(!target.exists());
}

#[tokio::test]
async fn deleting_stops_at_the_first_failure() {
    let mut t = test_engine();
    let (session, remote) = t.add_session("srv");
    remote.file("/a", b"", None).file("/c", b"", None);

    t.engine.handle_command(Command::Delete {
        location: Location::Session(session),
        paths: vec![PathBuf::from("/a"), PathBuf::from("/b"), PathBuf::from("/c")],
    });

    match t.next_event().await {
        Event::Notice { message, .. } => assert!(message.starts_with("Unable to delete:\n")),
        other => panic!("unexpected {other:?}"),
    }
    assert!(!remote.exists("/a"));
    assert!(remote.exists("/c"));
}
