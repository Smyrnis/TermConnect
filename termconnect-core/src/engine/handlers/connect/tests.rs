use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use termconnect_vfs::{
    Answer, Entry, Question, ShellInvocation,
    testing::{FakeFs, FakeProtocol},
};

use super::*;
use crate::{
    engine::{
        Command, PlanningScan,
        testing::{TestEngine, test_engine},
    },
    transfer::{Direction, JobStatus},
};

fn with_fake_profile(protocol: impl FnOnce(FakeFs) -> FakeProtocol) -> (TestEngine, FakeFs) {
    let mut t = test_engine();
    std::fs::create_dir_all(&t.engine.paths.config_dir).unwrap();
    std::fs::write(
        t.engine.paths.connections_file(),
        "[connections.srv]\nprotocol = \"fake\"\nhost = \"h\"\nusername = \"u\"\n",
    )
    .unwrap();
    let remote = FakeFs::new();
    t.engine.protocols = vec![Arc::new(protocol(remote.clone()))];
    (t, remote)
}

async fn next_matching<T>(t: &mut TestEngine, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
    loop {
        let event = t.next_event().await;
        if let Some(value) = pick(&event) {
            return value;
        }
    }
}

#[tokio::test]
async fn connecting_lists_the_remote_home() {
    let (mut t, remote) = with_fake_profile(FakeProtocol::new);
    remote.file("/home/user/notes.txt", b"hi", None);

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    t.run_internal().await;

    let session = next_matching(&mut t, |event| match event {
        Event::Connected { session, name, .. } if name == "srv" => Some(*session),
        _ => None,
    })
    .await;
    let names = next_matching(&mut t, |event| match event {
        Event::Listed { location: Location::Session(id), entries, path } if *id == session => {
            assert_eq!(path, &PathBuf::from("/home/user"));
            Some(entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>())
        }
        _ => None,
    })
    .await;
    assert_eq!(names, ["notes.txt"]);
}

#[tokio::test]
async fn connecting_announces_the_attempt_first() {
    let (mut t, _remote) = with_fake_profile(FakeProtocol::new);

    t.engine.handle_command(Command::Connect { profile: "srv".into() });

    assert!(matches!(t.next_event().await, Event::Connecting { name } if name == "srv"));
}

#[tokio::test]
async fn cancelling_the_password_prompt_fails_with_connection_cancelled_and_allows_a_retry() {
    let (mut t, _remote) = with_fake_profile(|fs| FakeProtocol::new(fs).requiring_password("pw"));

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    let request_id = next_matching(&mut t, |event| match event {
        Event::Question { request_id, question: Question::Password { .. } } => Some(*request_id),
        _ => None,
    })
    .await;
    t.engine.handle_command(Command::Answer { request_id, answer: None });
    t.run_internal().await;
    let message = next_matching(&mut t, |event| match event {
        Event::ConnectFailed { message, .. } => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(message, "Connection cancelled");
    assert!(t.engine.sessions.is_empty());

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    let request_id = next_matching(&mut t, |event| match event {
        Event::Question { request_id, .. } => Some(*request_id),
        _ => None,
    })
    .await;
    t.engine.handle_command(Command::Answer { request_id, answer: Some(Answer::Password("pw".into())) });
    t.run_internal().await;
    next_matching(&mut t, |event| matches!(event, Event::Connected { .. }).then_some(())).await;
}

#[tokio::test]
async fn a_wrong_password_reports_authentication_failed() {
    let (mut t, _remote) = with_fake_profile(|fs| FakeProtocol::new(fs).requiring_password("pw"));

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    let request_id = next_matching(&mut t, |event| match event {
        Event::Question { request_id, .. } => Some(*request_id),
        _ => None,
    })
    .await;
    t.engine.handle_command(Command::Answer { request_id, answer: Some(Answer::Password("no".into())) });
    t.run_internal().await;

    let message = next_matching(&mut t, |event| match event {
        Event::ConnectFailed { message, .. } => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(message, "Authentication failed for srv");
}

#[tokio::test]
async fn an_unreachable_host_reports_unable_to_connect() {
    let (mut t, _remote) = with_fake_profile(|fs| FakeProtocol::new(fs).failing_connect("Connection refused"));

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    t.run_internal().await;

    let message = next_matching(&mut t, |event| match event {
        Event::ConnectFailed { message, .. } => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(message, "Unable to connect to srv:\nConnection refused");
}

#[tokio::test]
async fn connecting_to_an_already_connected_profile_reuses_the_session() {
    let mut t = test_engine();
    let (session, _) = t.add_session("srv");

    t.engine.handle_command(Command::Connect { profile: "srv".into() });

    assert!(matches!(t.drain().as_slice(), [Event::Connected { session: id, .. }] if *id == session));
    assert_eq!(t.engine.sessions.len(), 1);
}

#[test]
fn an_unknown_profile_fails_without_connecting() {
    let mut t = test_engine();

    t.engine.handle_command(Command::Connect { profile: "ghost".into() });

    assert!(matches!(t.drain().as_slice(), [Event::ConnectFailed { name, .. }] if name == "ghost"));
}

#[test]
fn a_profile_whose_protocol_is_missing_fails_with_a_clear_message() {
    let (mut t, _remote) = with_fake_profile(FakeProtocol::new);
    t.engine.protocols.clear();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });

    let events = t.drain();
    assert!(
        matches!(events.as_slice(), [Event::ConnectFailed { message, .. }] if message == "No \"fake\" protocol is available for srv")
    );
}

fn enqueue(t: &mut TestEngine, session: u64, name: &str) -> u64 {
    t.engine.transfers.enqueue(
        session,
        Direction::Upload,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        10,
        None,
    )
}

fn messages(t: &mut TestEngine) -> Vec<String> {
    t.notices().into_iter().map(|(_, message)| message).collect()
}

#[test]
fn disconnect_reports_a_confirmation_notification() {
    let mut t = test_engine();
    let (session, _) = t.add_session("test");

    t.engine.handle_command(Command::Disconnect { session });

    assert!(t.engine.sessions.is_empty());
    assert_eq!(messages(&mut t), vec!["Disconnected from test".to_string()]);
}

#[test]
fn disconnecting_announces_the_session_is_gone() {
    let mut t = test_engine();
    let (session, _) = t.add_session("test");

    t.engine.handle_command(Command::Disconnect { session });

    assert!(
        t.drain().iter().any(
            |event| matches!(event, Event::Disconnected { session: id, name } if *id == session && name == "test")
        )
    );
}

#[test]
fn disconnecting_a_session_fails_its_queued_jobs_with_one_aggregated_notification() {
    let mut t = test_engine();
    let (session, _) = t.add_session("test");
    let job_a = enqueue(&mut t, session, "a.txt");
    let job_b = enqueue(&mut t, session, "b.txt");
    let other_session_job = enqueue(&mut t, 999, "c.txt");

    t.engine.disconnect(session);

    assert!(matches!(t.engine.transfers.get(job_a).unwrap().status, JobStatus::Failed(_)));
    assert!(matches!(t.engine.transfers.get(job_b).unwrap().status, JobStatus::Failed(_)));
    assert_eq!(t.engine.transfers.get(other_session_job).unwrap().status, JobStatus::Queued);
    let messages = messages(&mut t);
    let transfer_messages: Vec<&String> =
        messages.iter().filter(|m| m.contains("transfer") && m.contains("disconnected")).collect();
    assert_eq!(transfer_messages.len(), 1, "expected exactly one aggregated transfer notification, got {messages:?}");
    assert!(transfer_messages[0].contains('2'));
}

#[test]
fn disconnecting_a_session_counts_its_active_jobs_in_the_aggregated_notification() {
    let mut t = test_engine();
    let (session, _) = t.add_session("test");
    let active = enqueue(&mut t, session, "a.txt");
    t.engine.transfers.get_mut(active).unwrap().status = JobStatus::InProgress;
    let cancel = Arc::new(AtomicBool::new(false));
    t.engine.transfer_cancels.insert(active, cancel.clone());
    enqueue(&mut t, session, "b.txt");

    t.engine.disconnect(session);

    assert!(cancel.load(Ordering::Relaxed));
    let first = messages(&mut t).remove(0);
    assert!(first.contains("2 transfers cancelled"), "got {first}");
}

#[test]
fn disconnecting_a_session_counts_its_scans_in_the_aggregated_notification() {
    let mut t = test_engine();
    let (session, _) = t.add_session("test");
    let scan_cancel = Arc::new(AtomicBool::new(false));
    t.engine.planning.push(PlanningScan {
        batch_id: 0,
        session_id: session,
        direction: Direction::Upload,
        display_name: "myfolder".to_string(),
        cancel: scan_cancel.clone(),
    });
    enqueue(&mut t, session, "a.txt");

    t.engine.disconnect(session);

    assert!(scan_cancel.load(Ordering::Relaxed));
    let first = messages(&mut t).remove(0);
    assert!(first.contains("2 transfers cancelled"), "got {first}");
}

#[test]
fn disconnecting_drops_that_sessions_waiting_copies_and_counts_them() {
    let mut t = test_engine();
    let (session, _) = t.add_session("test");
    let batch_id = t.engine.transfers.start_batch("copy".to_string());
    let mut conflicting = crate::transfer::plan::PlannedFile {
        source: PathBuf::from("/local/a.txt"),
        destination: PathBuf::from("/remote/a.txt"),
        display_name: "a.txt".to_string(),
        size: 1,
        existing: None,
        source_modified: None,
        partial: None,
        resume: false,
    };
    conflicting.existing = Some(crate::transfer::plan::ExistingFile { size: 2, modified: None, is_dir: false });
    t.engine.review_or_apply_plan(
        batch_id,
        session,
        Direction::Upload,
        crate::transfer::plan::DirectoryPlan {
            files: vec![conflicting],
            skipped_symlinks: 0,
            taken_names: Default::default(),
        },
    );
    t.drain();

    t.engine.disconnect(session);

    assert!(t.engine.reviews.is_empty());
    let events = t.drain();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ConflictsWithdrawn { batch_ids } if batch_ids == &vec![batch_id]))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice { message, .. } if message.contains("1 transfer cancelled")))
    );
}

#[tokio::test]
async fn disconnecting_during_a_copy_cancels_it_and_late_results_do_not_bring_it_back() {
    let mut t = test_engine();
    let (session, _remote) = t.add_session("srv");
    let entries: Vec<Entry> = ["a", "b", "c"]
        .iter()
        .map(|name| {
            std::fs::write(t.dir.path().join(name), vec![0u8; 64]).unwrap();
            Entry { name: name.to_string(), path: t.dir.path().join(name), is_dir: false, size: 64, permissions: None }
        })
        .collect();

    t.engine.handle_command(Command::Copy {
        from: Location::Local,
        entries,
        to: Location::Session(session),
        dest_dir: PathBuf::from("/home/user"),
    });
    t.engine.handle_command(Command::Disconnect { session });
    t.run_internal().await;

    let messages = messages(&mut t);
    assert!(messages.iter().any(|m| m == "1 transfer cancelled \u{2014} session disconnected"), "{messages:?}");
    assert!(t.engine.sessions.is_empty());
    assert_eq!(t.engine.transfers.jobs().count(), 0);
    assert!(t.engine.planning.is_empty());
    assert!(t.engine.snapshot().active.is_empty());
}

#[test]
fn preparing_a_shell_hands_back_the_protocols_invocation() {
    let mut t = test_engine();
    let invocation = ShellInvocation { program: "ssh".into(), args: vec!["h".into()], env: Vec::new() };
    let session = t.add_session_with("srv", Arc::new(FakeFs::new()));
    t.engine.sessions.get_mut(&session).unwrap().protocol =
        Arc::new(FakeProtocol::new(FakeFs::new()).with_shell(invocation.clone()));

    t.engine.handle_command(Command::PrepareShell { session });

    assert!(matches!(t.drain().as_slice(), [Event::ShellReady { invocation: ready, .. }] if *ready == invocation));
}

#[test]
fn preparing_a_shell_for_a_protocol_without_one_warns() {
    let mut t = test_engine();
    let (session, _) = t.add_session("srv");

    t.engine.handle_command(Command::PrepareShell { session });

    assert_eq!(
        t.notices(),
        vec![(Severity::Warning, "This connection doesn't support a terminal session".to_string())]
    );
}
