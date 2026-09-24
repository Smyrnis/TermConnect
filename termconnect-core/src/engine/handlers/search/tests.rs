use std::path::PathBuf;

use super::*;
use crate::engine::{Command, testing::test_engine};

fn found_names(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::SearchFound(entry) => Some(entry.name.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn typing_a_pattern_streams_matching_results_back() {
    let mut t = test_engine();
    let (session, remote) = t.add_session("srv");
    remote.file("/home/user/target.log", b"", None).file("/home/user/other.txt", b"", None);

    t.engine.handle_command(Command::Search {
        location: Location::Session(session),
        root: PathBuf::from("/home/user"),
        pattern: "*target*".to_string(),
    });

    let mut events = Vec::new();
    loop {
        let event = t.next_event().await;
        let done = matches!(event, Event::SearchDone { .. });
        events.push(event);
        if done {
            break;
        }
    }
    assert_eq!(found_names(&events), ["target.log"]);
}

#[tokio::test(start_paused = true)]
async fn rapid_pattern_changes_dispatch_only_one_search_for_the_final_pattern() {
    let mut t = test_engine();
    let (session, remote) = t.add_session("srv");
    remote.file("/home/user/aaa.log", b"", None).file("/home/user/bbb.log", b"", None);
    let location = Location::Session(session);

    for pattern in ["*b*", "*bb*", "*a*"] {
        t.engine.handle_command(Command::Search {
            location,
            root: PathBuf::from("/home/user"),
            pattern: pattern.to_string(),
        });
    }
    tokio::time::sleep(SEARCH_DEBOUNCE * 2).await;

    let events = t.drain();
    let done = events.iter().filter(|event| matches!(event, Event::SearchDone { .. })).count();
    assert_eq!(done, 1, "expected exactly one dispatched search, got {events:?}");
    assert_eq!(found_names(&events), ["aaa.log"]);
}

#[tokio::test(start_paused = true)]
async fn cancelling_before_the_debounce_sends_nothing() {
    let mut t = test_engine();
    let (session, remote) = t.add_session("srv");
    remote.file("/home/user/aaa.log", b"", None);

    t.engine.handle_command(Command::Search {
        location: Location::Session(session),
        root: PathBuf::from("/home/user"),
        pattern: "*a*".to_string(),
    });
    t.engine.handle_command(Command::CancelSearch);
    tokio::time::sleep(SEARCH_DEBOUNCE * 2).await;

    assert!(t.drain().is_empty());
}
