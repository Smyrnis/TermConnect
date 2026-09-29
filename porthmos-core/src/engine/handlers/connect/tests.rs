use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use porthmos_vfs::{
    Answer, Entry, Question, ShellInvocation,
    testing::{FakeFs, FakeProtocol},
};

use super::*;
use crate::{
    engine::{
        Command, Location, PlanningScan,
        testing::{TestEngine, test_engine, test_engine_with_secrets},
    },
    profiles::store,
    secrets::TestBackend,
    transfer::{Direction, JobStatus},
};

fn with_fake_profile(protocol: impl FnOnce(FakeFs) -> FakeProtocol) -> (TestEngine, FakeFs) {
    with_fake_profile_lines("", protocol)
}

fn with_fake_profile_lines(extra_lines: &str, protocol: impl FnOnce(FakeFs) -> FakeProtocol) -> (TestEngine, FakeFs) {
    let mut t = test_engine();
    std::fs::create_dir_all(&t.engine.paths.config_dir).unwrap();
    std::fs::write(
        t.engine.paths.connections_file(),
        format!("[connections.srv]\nprotocol = \"fake\"\nhost = \"h\"\nusername = \"u\"\n{extra_lines}"),
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

async fn first_session_listing(t: &mut TestEngine) -> (PathBuf, Vec<String>) {
    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    t.run_internal().await;
    let session = next_matching(t, |event| match event {
        Event::Connected { session, .. } => Some(*session),
        _ => None,
    })
    .await;
    next_matching(t, |event| match event {
        Event::Listed { location: Location::Session(id), path, entries } if *id == session => {
            Some((path.clone(), entries.iter().map(|entry| entry.name.clone()).collect()))
        }
        _ => None,
    })
    .await
}

#[tokio::test]
async fn connecting_opens_the_profiles_absolute_remote_path() {
    let (mut t, remote) = with_fake_profile_lines("remote_path = \"/var/www\"\n", FakeProtocol::new);
    remote.file("/var/www/index.html", b"<html>", None);

    let (path, names) = first_session_listing(&mut t).await;

    assert_eq!(path, PathBuf::from("/var/www"));
    assert_eq!(names, ["index.html"]);
}

#[tokio::test]
async fn connecting_opens_a_relative_remote_path_under_the_remote_home() {
    let (mut t, remote) = with_fake_profile_lines("remote_path = \"projects\"\n", FakeProtocol::new);
    remote.file("/home/user/projects/app.rs", b"fn main() {}", None);

    let (path, names) = first_session_listing(&mut t).await;

    assert_eq!(path, PathBuf::from("/home/user/projects"));
    assert_eq!(names, ["app.rs"]);
}

#[tokio::test]
async fn connecting_opens_a_tilde_remote_path_under_the_remote_home() {
    let (mut t, remote) = with_fake_profile_lines("remote_path = \"~/projects\"\n", FakeProtocol::new);
    remote.dir("/home/user/projects");

    let (path, _) = first_session_listing(&mut t).await;

    assert_eq!(path, PathBuf::from("/home/user/projects"));
}

#[tokio::test]
async fn a_missing_remote_path_warns_and_falls_back_to_the_remote_home() {
    let (mut t, remote) = with_fake_profile_lines("remote_path = \"/var/www\"\n", FakeProtocol::new);
    remote.file("/home/user/notes.txt", b"hi", None);

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    t.run_internal().await;
    let warning = next_matching(&mut t, |event| match event {
        Event::Notice { severity: Severity::Warning, message } => Some(message.clone()),
        Event::Listed { .. } => panic!("listed before warning about the missing remote path"),
        _ => None,
    })
    .await;
    let (path, names) = next_matching(&mut t, |event| match event {
        Event::Listed { location: Location::Session(_), path, entries } => {
            Some((path.clone(), entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>()))
        }
        _ => None,
    })
    .await;

    assert!(warning.starts_with("Unable to open /var/www on srv, showing the home directory instead:\n"), "{warning}");
    assert_eq!(path, PathBuf::from("/home/user"));
    assert_eq!(names, ["notes.txt"]);
}

#[tokio::test]
async fn an_absolute_remote_path_opens_even_when_the_remote_home_is_unavailable() {
    let (mut t, remote) = with_fake_profile_lines("remote_path = \"/var/www\"\n", FakeProtocol::new);
    remote.file("/var/www/index.html", b"<html>", None).fail_home();

    let (path, names) = first_session_listing(&mut t).await;

    assert_eq!(path, PathBuf::from("/var/www"));
    assert_eq!(names, ["index.html"]);
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
    t.engine.handle_command(Command::Answer { request_id, answer: None, save: false });
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
    t.engine.handle_command(Command::Answer { request_id, answer: Some(Answer::Password("pw".into())), save: false });
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
    t.engine.handle_command(Command::Answer { request_id, answer: Some(Answer::Password("no".into())), save: false });
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

#[test]
fn connecting_to_a_missing_ssh_host_fails_with_a_message() {
    let mut t = test_engine();
    crate::profiles::store::save_ssh_labels(
        &t.engine.paths,
        "old",
        &crate::profiles::Labels { group: Some("A".into()), tags: Vec::new(), in_keyring: Vec::new() },
    )
    .unwrap();

    t.engine.handle_command(Command::Connect { profile: "old".into() });

    assert!(t.drain().iter().any(|event| matches!(
        event,
        Event::ConnectFailed { name, message } if name == "old" && message == "old is no longer in ~/.ssh/config"
    )));
}

#[tokio::test]
async fn a_profile_wins_over_shadowed_labels_of_the_same_name() {
    let (mut t, _remote) = with_fake_profile_lines("[ssh_hosts.srv]\ngroup = \"A\"\n", |fs| {
        FakeProtocol::new(fs).with_discovered(vec![porthmos_vfs::Target {
            name: "srv".into(),
            host: "srv.example".into(),
            port: 22,
            username: "u".into(),
            password: None,
            options: Default::default(),
        }])
    });

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    t.run_internal().await;

    next_matching(&mut t, |event| match event {
        Event::Connected { name, .. } if name == "srv" => Some(()),
        Event::ConnectFailed { message, .. } => panic!("connect failed: {message}"),
        _ => None,
    })
    .await;
}

fn with_secured_profile_lines(
    backend: &Arc<TestBackend>, extra_lines: &str, protocol: impl FnOnce(FakeFs) -> FakeProtocol,
) -> TestEngine {
    let mut t = test_engine_with_secrets(backend.clone());
    std::fs::create_dir_all(&t.engine.paths.config_dir).unwrap();
    std::fs::write(
        t.engine.paths.connections_file(),
        format!("[connections.srv]\nprotocol = \"fake\"\nhost = \"h\"\nusername = \"u\"\n{extra_lines}"),
    )
    .unwrap();
    t.engine.protocols = vec![Arc::new(protocol(FakeFs::new()))];
    t
}

fn with_secured_profile(backend: &Arc<TestBackend>, protocol: impl FnOnce(FakeFs) -> FakeProtocol) -> TestEngine {
    with_secured_profile_lines(backend, "", protocol)
}

fn discovered_host(name: &str) -> porthmos_vfs::Target {
    porthmos_vfs::Target {
        name: name.into(),
        host: format!("{name}.example"),
        port: 22,
        username: "u".into(),
        password: None,
        options: Default::default(),
    }
}

async fn answer_next_question(t: &mut TestEngine, password: &str, save: bool, seen: &mut Vec<Event>) {
    let request_id = loop {
        let event = t.next_event().await;
        let found = match &event {
            Event::Question { request_id, question: Question::Password { .. } } => Some(*request_id),
            _ => None,
        };
        seen.push(event);
        if let Some(request_id) = found {
            break request_id;
        }
    };
    t.engine.handle_command(Command::Answer { request_id, answer: Some(Answer::Password(password.into())), save });
}

async fn finish(t: &mut TestEngine, seen: &mut Vec<Event>) {
    t.settle().await;
    seen.extend(t.drain());
}

fn connected(seen: &[Event], name: &str) -> Option<crate::engine::SessionId> {
    seen.iter().rev().find_map(|event| match event {
        Event::Connected { session, name: connected, .. } if connected == name => Some(*session),
        _ => None,
    })
}

fn questions(seen: &[Event]) -> usize {
    seen.iter().filter(|event| matches!(event, Event::Question { question: Question::Password { .. }, .. })).count()
}

fn errors(seen: &[Event]) -> Vec<String> {
    seen.iter()
        .filter_map(|event| match event {
            Event::Notice { severity: Severity::Error, message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

fn markers(t: &TestEngine) -> Vec<String> {
    store::load(&t.engine.paths).unwrap().remove(0).in_keyring
}

#[tokio::test]
async fn a_prompted_password_with_save_is_stored_after_the_login_succeeds() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("hunter2"));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "hunter2", true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert!(connected(&seen, "srv").is_some());
    assert_eq!(backend.stored("profile:srv").as_deref(), Some("hunter2"));
    assert_eq!(backend.calls(), vec!["set profile:srv"]);
    assert_eq!(markers(&t), vec!["password"]);
    assert!(errors(&seen).is_empty());
    t.assert_secret_nowhere("hunter2", &seen);
}

#[tokio::test]
async fn without_save_the_password_is_kept_for_the_run_and_not_asked_again() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("hunter2"));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "hunter2", false, &mut seen).await;
    finish(&mut t, &mut seen).await;
    let session = connected(&seen, "srv").unwrap();
    t.engine.handle_command(Command::Disconnect { session });
    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    finish(&mut t, &mut seen).await;

    assert_eq!(questions(&seen), 1);
    assert_eq!(seen.iter().filter(|event| matches!(event, Event::Connected { .. })).count(), 2);
    assert!(backend.calls().is_empty());
    assert!(markers(&t).is_empty());
    t.assert_secret_nowhere("hunter2", &seen);
}

#[tokio::test]
async fn a_rejected_password_is_never_cached() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("pw"));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "wrong", true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert!(seen.iter().any(|event| matches!(event, Event::ConnectFailed { .. })));
    assert!(t.engine.secrets.cached("profile:srv").is_none());
    assert!(backend.calls().is_empty());
    assert!(markers(&t).is_empty());
}

#[tokio::test]
async fn after_a_rejected_try_only_the_accepted_password_is_stored() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("right"));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "wrong", true, &mut seen).await;
    finish(&mut t, &mut seen).await;
    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "right", true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(backend.stored("profile:srv").as_deref(), Some("right"));
    assert_eq!(backend.calls(), vec!["set profile:srv"]);
    t.assert_secret_nowhere("wrong", &seen);
}

#[tokio::test]
async fn a_cancelled_prompt_stores_nothing() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("pw"));

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    let request_id = next_matching(&mut t, |event| match event {
        Event::Question { request_id, .. } => Some(*request_id),
        _ => None,
    })
    .await;
    t.engine.handle_command(Command::Answer { request_id, answer: None, save: true });
    t.settle().await;

    assert!(t.engine.secrets.cached("profile:srv").is_none());
    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn a_saved_password_connects_without_asking_and_reads_the_keyring_once() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:srv", "hunter2");
    let mut t = with_secured_profile_lines(&backend, "in_keyring = [\"password\"]\n", |fs| {
        FakeProtocol::new(fs).requiring_password("hunter2")
    });
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    finish(&mut t, &mut seen).await;
    let session = connected(&seen, "srv").unwrap();
    t.engine.handle_command(Command::Disconnect { session });
    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    finish(&mut t, &mut seen).await;

    assert_eq!(questions(&seen), 0);
    assert_eq!(seen.iter().filter(|event| matches!(event, Event::Connected { .. })).count(), 2);
    assert_eq!(backend.calls(), vec!["get profile:srv"]);
    t.assert_secret_nowhere("hunter2", &seen);
}

#[tokio::test]
async fn a_marker_without_a_keyring_entry_just_asks() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile_lines(&backend, "in_keyring = [\"password\"]\n", |fs| {
        FakeProtocol::new(fs).requiring_password("pw")
    });
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "pw", false, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert!(connected(&seen, "srv").is_some());
    assert_eq!(backend.calls(), vec!["get profile:srv"]);
    assert!(errors(&seen).is_empty());
}

#[tokio::test]
async fn a_saved_but_outdated_password_is_asked_again_and_replaced() {
    let backend = Arc::new(TestBackend::new());
    backend.put("profile:srv", "old");
    let mut t = with_secured_profile_lines(&backend, "in_keyring = [\"password\"]\n", |fs| {
        FakeProtocol::new(fs).requiring_password("new")
    });
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "new", true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(backend.stored("profile:srv").as_deref(), Some("new"));
    assert_eq!(backend.calls(), vec!["get profile:srv", "set profile:srv"]);
    assert_eq!(markers(&t), vec!["password"]);
    assert_eq!(t.engine.secrets.cached("profile:srv").as_deref().map(String::as_str), Some("new"));
}

#[tokio::test]
async fn a_failing_keyring_write_keeps_the_password_for_the_run() {
    let backend = Arc::new(TestBackend::new());
    backend.fail_writes();
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("pw"));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "pw", true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(
        errors(&seen),
        vec!["Couldn't save the password for srv in the system keyring: write failed".to_string()]
    );
    assert!(markers(&t).is_empty());
    assert_eq!(t.engine.secrets.cached("profile:srv").as_deref().map(String::as_str), Some("pw"));
    assert!(connected(&seen, "srv").is_some());
}

#[tokio::test]
async fn saving_without_a_keyring_keeps_the_password_for_the_run_quietly() {
    let backend = Arc::new(TestBackend::new());
    backend.fail_reads();
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("pw"));
    assert!(!t.engine.secrets.probe().await);
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "pw", true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(backend.calls(), vec!["get porthmos:probe"]);
    assert!(errors(&seen).is_empty());
    assert!(markers(&t).is_empty());
    assert_eq!(t.engine.secrets.cached("profile:srv").as_deref().map(String::as_str), Some("pw"));
}

#[tokio::test]
async fn a_marker_with_no_keyring_available_asks_without_calling_it() {
    let backend = Arc::new(TestBackend::new());
    backend.fail_reads();
    let mut t = with_secured_profile_lines(&backend, "in_keyring = [\"password\"]\n", |fs| {
        FakeProtocol::new(fs).requiring_password("pw")
    });
    assert!(!t.engine.secrets.probe().await);
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "pw", false, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(backend.calls(), vec!["get porthmos:probe"]);
    assert!(connected(&seen, "srv").is_some());
}

#[tokio::test]
async fn an_ssh_config_host_saves_under_its_alias_and_uses_it_next_time() {
    let backend = Arc::new(TestBackend::new());
    let mut t = test_engine_with_secrets(backend.clone());
    t.engine.protocols = vec![Arc::new(
        FakeProtocol::new(FakeFs::new()).requiring_password("pw").with_discovered(vec![discovered_host("web1")]),
    )];
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "web1".into() });
    answer_next_question(&mut t, "pw", true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(backend.stored("ssh:web1").as_deref(), Some("pw"));
    assert!(backend.stored("profile:web1").is_none());
    assert_eq!(store::load_ssh_labels(&t.engine.paths).unwrap()["web1"].in_keyring, vec!["password"]);
    assert!(store::load(&t.engine.paths).unwrap().is_empty());
    t.assert_secret_nowhere("pw\"", &seen);

    let mut fresh = test_engine_with_secrets(backend.clone());
    std::fs::create_dir_all(&fresh.engine.paths.config_dir).unwrap();
    std::fs::copy(t.engine.paths.connections_file(), fresh.engine.paths.connections_file()).unwrap();
    fresh.engine.protocols = vec![Arc::new(
        FakeProtocol::new(FakeFs::new()).requiring_password("pw").with_discovered(vec![discovered_host("web1")]),
    )];
    let mut again = Vec::new();
    fresh.engine.handle_command(Command::Connect { profile: "web1".into() });
    finish(&mut fresh, &mut again).await;
    assert_eq!(questions(&again), 0);
    assert!(connected(&again, "web1").is_some());
}

#[tokio::test]
async fn a_password_with_spaces_and_unicode_round_trips() {
    let backend = Arc::new(TestBackend::new());
    let password = " p ä: ß 🔑 ";
    let mut t = with_secured_profile(&backend, move |fs| FakeProtocol::new(fs).requiring_password(password));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, password, true, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(backend.stored("profile:srv").as_deref(), Some(password));
    assert_eq!(t.engine.secrets.cached("profile:srv").as_deref().map(String::as_str), Some(password));
    let session = connected(&seen, "srv").unwrap();
    assert_eq!(t.engine.shell_target(session).and_then(|target| target.password).as_deref(), Some(password));
}

#[tokio::test]
async fn a_saved_secret_option_reaches_the_protocol() {
    let backend = Arc::new(TestBackend::new());
    backend.put("option:token:srv", "tok-123");
    let protocol = FakeProtocol::new(FakeFs::new()).with_form(token_form());
    let seen_targets = protocol.seen_targets();
    let mut t = with_secured_profile_lines(&backend, "in_keyring = [\"token\"]\n", move |_| protocol);
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    finish(&mut t, &mut seen).await;

    let target = seen_targets.lock().unwrap().clone().unwrap();
    assert_eq!(target.options.get("token").map(String::as_str), Some("tok-123"));
    assert_eq!(target.password, None);
    assert_eq!(backend.calls(), vec!["get option:token:srv"]);
    let session = connected(&seen, "srv").unwrap();
    assert!(!t.engine.sessions[&session].entry.options.contains_key("token"));
}

#[tokio::test]
async fn the_shell_handoff_gets_the_password_but_the_session_does_not_keep_it() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| {
        FakeProtocol::new(fs).requiring_password("pw").with_shell(ShellInvocation {
            program: "ssh".into(),
            args: Vec::new(),
            env: Vec::new(),
        })
    });
    let mut seen = Vec::new();
    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "pw", false, &mut seen).await;
    finish(&mut t, &mut seen).await;
    let session = connected(&seen, "srv").unwrap();

    assert!(t.engine.sessions[&session].entry.password.is_none());
    assert_eq!(t.engine.shell_target(session).and_then(|target| target.password).as_deref(), Some("pw"));
    assert!(!format!("{:?}", t.engine.sessions[&session].entry).contains("pw\""));
}

#[test]
fn an_answer_never_shows_the_password_when_printed() {
    let command = Command::Answer { request_id: 1, answer: Some(Answer::Password("hunter2".into())), save: true };
    assert!(!format!("{command:?}").contains("hunter2"));
}

fn token_form() -> porthmos_vfs::ConnectionForm {
    let mut form = porthmos_vfs::ConnectionForm::standard(22);
    form.options.push(porthmos_vfs::OptionField {
        key: "token",
        label: "Token",
        required: false,
        kind: porthmos_vfs::OptionKind::Secret,
    });
    form
}

#[tokio::test]
async fn two_connects_to_the_same_profile_ask_only_once() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("pw"));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "pw", false, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert_eq!(questions(&seen), 1);
    assert_eq!(seen.iter().filter(|event| matches!(event, Event::Connected { .. })).count(), 1);
    assert_eq!(t.engine.sessions.len(), 1);
}

#[tokio::test]
async fn a_failed_connect_allows_a_new_attempt() {
    let backend = Arc::new(TestBackend::new());
    let mut t = with_secured_profile(&backend, |fs| FakeProtocol::new(fs).requiring_password("pw"));
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "wrong", false, &mut seen).await;
    finish(&mut t, &mut seen).await;
    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    answer_next_question(&mut t, "pw", false, &mut seen).await;
    finish(&mut t, &mut seen).await;

    assert!(connected(&seen, "srv").is_some());
    assert_eq!(questions(&seen), 2);
}

#[tokio::test]
async fn a_secret_option_kept_only_for_the_run_reaches_the_protocol() {
    let protocol = FakeProtocol::new(FakeFs::new()).with_form(token_form());
    let seen_targets = protocol.seen_targets();
    let (mut t, _remote) = with_fake_profile(move |_| protocol);
    t.engine.secrets.remember("option:token:srv", "tok-run");
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    finish(&mut t, &mut seen).await;

    let target = seen_targets.lock().unwrap().clone().unwrap();
    assert_eq!(target.options.get("token").map(String::as_str), Some("tok-run"));
}

#[tokio::test]
async fn an_old_plaintext_secret_option_is_never_given_to_the_protocol() {
    let protocol = FakeProtocol::new(FakeFs::new()).with_form(token_form());
    let seen_targets = protocol.seen_targets();
    let (mut t, _remote) = with_fake_profile_lines("token = \"plain-old\"\n", move |_| protocol);
    let mut seen = Vec::new();

    t.engine.handle_command(Command::Connect { profile: "srv".into() });
    finish(&mut t, &mut seen).await;

    let target = seen_targets.lock().unwrap().clone().unwrap();
    assert!(!target.options.contains_key("token"));
    assert!(!format!("{seen:?}").contains("plain-old"));
}
