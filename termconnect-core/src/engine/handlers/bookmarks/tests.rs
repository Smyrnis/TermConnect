use std::path::PathBuf;

use super::*;
use crate::engine::{Command, testing::test_engine};

fn bookmark_events(events: Vec<Event>) -> Vec<Vec<Bookmark>> {
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::Bookmarks(bookmarks) => Some(bookmarks),
            _ => None,
        })
        .collect()
}

#[test]
fn a_local_bookmark_is_saved_and_published() {
    let mut t = test_engine();

    t.engine.handle_command(Command::AddBookmark {
        label: "here".to_string(),
        location: Location::Local,
        path: PathBuf::from("/data"),
    });

    let published = bookmark_events(t.drain());
    assert_eq!(published, vec![vec![Bookmark { label: "here".into(), path: PathBuf::from("/data"), host: None }]]);
    let (reloaded, _) = bookmarks::load(&t.engine.paths).unwrap();
    assert_eq!(reloaded.len(), 1);
}

#[test]
fn a_remote_bookmark_remembers_the_connection_name() {
    let mut t = test_engine();
    let (session, _) = t.add_session("production");

    t.engine.handle_command(Command::AddBookmark {
        label: "etc".to_string(),
        location: Location::Session(session),
        path: PathBuf::from("/etc"),
    });

    let published = bookmark_events(t.drain());
    assert_eq!(published[0][0].host.as_deref(), Some("production"));
}

#[test]
fn a_remote_bookmark_without_the_session_warns() {
    let mut t = test_engine();

    t.engine.handle_command(Command::AddBookmark {
        label: "etc".to_string(),
        location: Location::Session(9),
        path: PathBuf::from("/etc"),
    });

    assert_eq!(t.notices(), vec![(Severity::Warning, "Connect to a remote server first".to_string())]);
}

#[test]
fn removing_a_bookmark_deletes_it_and_says_so() {
    let mut t = test_engine();
    t.engine.handle_command(Command::AddBookmark {
        label: "a".to_string(),
        location: Location::Local,
        path: PathBuf::from("/a"),
    });
    t.drain();

    t.engine.handle_command(Command::RemoveBookmark { index: 0 });

    let events = t.drain();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice { message, .. } if message == "Removed bookmark \"a\""))
    );
    assert_eq!(bookmark_events(events), vec![Vec::<Bookmark>::new()]);
}
