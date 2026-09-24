mod support;

use std::path::PathBuf;

use porthmos_core::{Answer, Command, Entry, Event, Location, Question, transfer::rows::RowState};
use porthmos_vfs::testing::FakeProtocol;

#[tokio::test]
async fn the_core_starts_by_publishing_the_saved_bookmarks() {
    let mut harness = support::start();

    let bookmarks = harness
        .next(|event| match event {
            Event::Bookmarks(bookmarks) => Some(bookmarks.clone()),
            _ => None,
        })
        .await;

    assert!(bookmarks.is_empty());
}

#[tokio::test]
async fn connecting_lists_the_remote_home() {
    let mut harness = support::start();
    harness.remote.file("/home/user/notes.txt", b"hi", None);

    let session = harness.connect().await;
    let names = harness
        .next(|event| match event {
            Event::Listed { location: Location::Session(id), entries, .. } if *id == session => {
                Some(entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>())
            }
            _ => None,
        })
        .await;

    assert_eq!(names, ["notes.txt"]);
}

#[tokio::test]
async fn cancelling_the_password_prompt_fails_with_connection_cancelled_and_allows_a_retry() {
    let mut harness = support::start_with(|fs| FakeProtocol::new(fs).requiring_password("pw"));

    harness.core.send(Command::Connect { profile: "srv".into() });
    let request_id = harness
        .next(|event| match event {
            Event::Question { request_id, question: Question::Password { .. } } => Some(*request_id),
            _ => None,
        })
        .await;
    harness.core.send(Command::Answer { request_id, answer: None });
    let message = harness
        .next(|event| match event {
            Event::ConnectFailed { message, .. } => Some(message.clone()),
            _ => None,
        })
        .await;
    assert_eq!(message, "Connection cancelled");

    harness.core.send(Command::Connect { profile: "srv".into() });
    let request_id = harness
        .next(|event| match event {
            Event::Question { request_id, .. } => Some(*request_id),
            _ => None,
        })
        .await;
    harness.core.send(Command::Answer { request_id, answer: Some(Answer::Password("pw".into())) });
    harness.next(|event| matches!(event, Event::Connected { .. }).then_some(())).await;
}

#[tokio::test]
async fn a_download_lands_on_the_local_side_and_reports_progress() {
    let mut harness = support::start();
    harness.remote.file("/home/user/report.pdf", b"%PDF-1.7", Some(1));
    let session = harness.connect().await;

    harness.core.send(Command::Copy {
        from: Location::Session(session),
        entries: vec![Entry {
            name: "report.pdf".into(),
            path: PathBuf::from("/home/user/report.pdf"),
            is_dir: false,
            size: 8,
            permissions: None,
        }],
        to: Location::Local,
        dest_dir: harness.local.path().to_path_buf(),
    });
    let mut done = false;
    let mut local_changed = false;
    while !(done && local_changed) {
        let event = harness.next(|event| Some(event.clone())).await;
        match event {
            Event::TransfersChanged(snapshot) => {
                done |= snapshot.rows.first().is_some_and(|row| row.state == RowState::Done);
            }
            Event::LocationChanged { location: Location::Local } => local_changed = true,
            _ => {}
        }
    }

    assert_eq!(std::fs::read(harness.local.path().join("report.pdf")).unwrap(), b"%PDF-1.7");
}

#[tokio::test]
async fn a_protocol_without_a_shell_is_announced_on_connect() {
    let mut harness = support::start();

    harness.core.send(Command::Connect { profile: "srv".into() });
    let shell_available = harness
        .next(|event| match event {
            Event::Connected { shell_available, .. } => Some(*shell_available),
            _ => None,
        })
        .await;

    assert!(!shell_available);
}
