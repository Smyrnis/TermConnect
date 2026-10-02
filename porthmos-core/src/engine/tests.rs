use std::{path::PathBuf, time::Duration};

use tokio::sync::watch;

use super::*;
use crate::{
    engine::testing::{TestEngine, test_engine},
    history::{History, HistoryResult},
    tasks::Scope,
    transfer::{Direction, JobStatus, rows::RowKind},
};

fn queue_job(t: &mut TestEngine, session: u64, name: &str) -> u64 {
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

fn snapshots(events: Vec<Event>) -> Vec<TransferSnapshot> {
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::TransfersChanged(snapshot) => Some(snapshot),
            _ => None,
        })
        .collect()
}

#[test]
fn with_no_interval_every_request_publishes_at_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.request_publish();
    queue_job(&mut t, session, "b");
    t.engine.request_publish();

    assert_eq!(snapshots(t.drain()).len(), 2);
    assert!(!t.engine.transfers_dirty);
    assert!(t.engine.flush_deadline().is_none());
}

#[test]
fn a_burst_inside_the_interval_is_published_once_and_the_rest_waits_for_the_flush() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_millis(80);
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.request_publish();
    queue_job(&mut t, session, "b");
    t.engine.request_publish();
    queue_job(&mut t, session, "c");
    t.engine.request_publish();

    let first = snapshots(t.drain());
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].rows.len(), 1);
    assert!(t.engine.transfers_dirty);
    assert!(t.engine.flush_deadline().is_some());

    std::thread::sleep(Duration::from_millis(100));
    t.engine.publish_transfers();

    let flushed = snapshots(t.drain());
    assert_eq!(flushed.len(), 1);
    assert_eq!(flushed[0].rows.len(), 3);
    assert!(!t.engine.transfers_dirty);
    assert!(t.engine.flush_deadline().is_none());
}

#[test]
fn a_request_after_a_quiet_period_publishes_at_once() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_millis(40);
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.request_publish();
    t.drain();
    std::thread::sleep(Duration::from_millis(60));
    queue_job(&mut t, session, "b");

    t.engine.request_publish();

    assert_eq!(snapshots(t.drain()).len(), 1);
}

#[test]
fn nothing_is_published_when_nothing_changed() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    queue_job(&mut t, session, "a");
    t.engine.publish_transfers();
    t.drain();

    t.engine.publish_transfers();

    assert!(snapshots(t.drain()).is_empty());
}

#[tokio::test]
async fn the_run_loop_publishes_the_trailing_snapshot_of_a_burst() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_millis(60);
    let (session, _fs) = t.add_session("prod");
    let first = queue_job(&mut t, session, "a");
    let second = queue_job(&mut t, session, "b");
    queue_job(&mut t, session, "c");
    let TestEngine { engine, mut events, internal, dir } = t;
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    let running = tokio::spawn(engine.run(receiver, internal));

    commands.send(Command::CancelRow { kind: RowKind::Single(first) }).unwrap();
    commands.send(Command::CancelRow { kind: RowKind::Single(second) }).unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    commands.send(Command::Shutdown).unwrap();
    running.await.unwrap();

    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::TransfersChanged(snapshot) = event {
            seen.push(snapshot);
        }
    }
    assert_eq!(seen.len(), 2);
    let states: Vec<_> = seen[1].rows.iter().map(|row| row.state).collect();
    assert_eq!(
        states,
        vec![
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Queued
        ]
    );
    drop(dir);
}

#[test]
fn a_job_status_helper_still_reaches_the_published_snapshot() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;

    t.engine.publish_transfers();

    let published = snapshots(t.drain());
    assert_eq!(published[0].active.len(), 1);
}

#[tokio::test]
async fn shutting_down_while_a_snapshot_is_pending_publishes_it_first() {
    let mut t = test_engine();
    t.engine.publish_interval = Duration::from_secs(30);
    let (session, _fs) = t.add_session("prod");
    let first = queue_job(&mut t, session, "a");
    let second = queue_job(&mut t, session, "b");
    queue_job(&mut t, session, "c");
    let TestEngine { engine, mut events, internal, dir } = t;
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    let running = tokio::spawn(engine.run(receiver, internal));

    commands.send(Command::CancelRow { kind: RowKind::Single(first) }).unwrap();
    commands.send(Command::CancelRow { kind: RowKind::Single(second) }).unwrap();
    commands.send(Command::Shutdown).unwrap();
    running.await.unwrap();

    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::TransfersChanged(snapshot) = event {
            seen.push(snapshot);
        }
    }
    assert_eq!(seen.len(), 2);
    let states: Vec<_> = seen[1].rows.iter().map(|row| row.state).collect();
    assert_eq!(
        states,
        vec![
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Cancelled,
            crate::transfer::rows::RowState::Queued
        ]
    );
    drop(dir);
}

#[tokio::test]
async fn a_panicking_background_task_is_reported_with_its_name() {
    let mut t = test_engine();

    t.engine.tasks.spawn("crasher", Scope::Background, |_| async move {
        panic!("kaboom");
    });
    t.run_internal().await;

    let notices: Vec<String> = t
        .drain()
        .into_iter()
        .filter_map(|event| match event {
            Event::Notice { severity: Severity::Error, message } => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(notices, vec!["A background task failed: crasher".to_string()]);
}

#[tokio::test]
async fn cancelling_everything_marks_every_running_transfer_and_planning_scan() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let running = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(running).unwrap().status = JobStatus::InProgress;
    t.engine.planning.push(PlanningScan {
        batch_id: 5,
        session_id: session,
        direction: Direction::Upload,
        display_name: "scan".to_string(),
    });

    t.engine.cancel_all_work();

    assert!(t.engine.tasks.take_cancelled(Scope::Transfer(running)));
    assert!(t.engine.tasks.take_cancelled(Scope::Planning(5)));
}

struct RunningEngine {
    commands: tokio::sync::mpsc::UnboundedSender<Command>,
    running: tokio::task::JoinHandle<()>,
    finished: watch::Receiver<bool>,
    paths: crate::Paths,
    events: tokio::sync::mpsc::UnboundedReceiver<Event>,
    _dir: tempfile::TempDir,
}

impl RunningEngine {
    async fn shut_down(&mut self) -> Vec<Event> {
        self.commands.send(Command::Shutdown).unwrap();
        self.finished.wait_for(|done| *done).await.unwrap();
        (&mut self.running).await.unwrap();
        std::iter::from_fn(|| self.events.try_recv().ok()).collect()
    }
}

async fn run_until_shutdown(t: TestEngine, grace: Duration) -> RunningEngine {
    run_until_shutdown_waiting_for_uploads(t, grace, Duration::from_secs(2)).await
}

async fn run_until_shutdown_waiting_for_uploads(
    t: TestEngine, grace: Duration, upload_grace: Duration,
) -> RunningEngine {
    let TestEngine { mut engine, internal, dir, events } = t;
    engine.shutdown_grace = grace;
    engine.upload_grace = upload_grace;
    engine.flush_grace = Duration::from_millis(200);
    let (finished_tx, finished) = watch::channel(false);
    engine.finished = Some(finished_tx);
    let paths = engine.paths.clone();
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    let running = tokio::spawn(engine.run(receiver, internal));
    RunningEngine { commands, running, finished, paths, events, _dir: dir }
}

#[tokio::test]
async fn shutdown_records_interrupted_rows_before_it_reports_finished() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    let mut engine = run_until_shutdown(t, Duration::from_millis(200)).await;

    engine.shut_down().await;

    let entries = History::load(&engine.paths).0.entries().to_vec();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].result, HistoryResult::Interrupted);
}

#[tokio::test]
async fn shutdown_cancels_cooperative_tasks_and_aborts_stubborn_ones() {
    let t = test_engine();
    let cooperative = t.engine.tasks.spawn("cooperative", Scope::Background, |cancel| async move {
        while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    t.engine.tasks.spawn("stubborn", Scope::Background, |_| async move {
        std::future::pending::<()>().await;
    });
    let tasks = t.engine.tasks.clone();
    let mut engine = run_until_shutdown(t, Duration::from_millis(150)).await;
    let start = std::time::Instant::now();

    engine.shut_down().await;

    assert!(cooperative.load(std::sync::atomic::Ordering::Relaxed));
    assert!(tasks.live_names().is_empty());
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_dropped_command_channel_also_shuts_the_engine_down() {
    let t = test_engine();
    let RunningEngine { commands, running, mut finished, .. } = run_until_shutdown(t, Duration::from_millis(50)).await;

    drop(commands);
    finished.wait_for(|done| *done).await.unwrap();
    running.await.unwrap();
}

#[tokio::test]
async fn a_keyring_result_that_arrives_during_the_grace_is_still_applied() {
    let t = test_engine();
    let internal = t.engine.internal.clone();
    t.engine.tasks.spawn("late-keyring", Scope::Background, move |_| async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let _ = internal.send(Internal::KeyringDone(crate::engine::handlers::KeyringDone {
            job: 1,
            owner: crate::engine::handlers::SecretOwner::Profile("prod".to_string()),
            rollback: Vec::new(),
            restore: Vec::new(),
            errors: vec!["keyring write failed".to_string()],
        }));
    });
    let mut engine = run_until_shutdown(t, Duration::from_millis(400)).await;

    let events = engine.shut_down().await;

    let messages: Vec<String> = events
        .into_iter()
        .filter_map(|event| match event {
            Event::Notice { severity: Severity::Error, message } => Some(message),
            _ => None,
        })
        .collect();
    assert!(messages.contains(&"keyring write failed".to_string()), "{messages:?}");
}

#[tokio::test]
async fn a_transfer_result_that_arrives_during_the_grace_is_not_acted_on() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    let internal = t.engine.internal.clone();
    t.engine.tasks.spawn("late-transfer", Scope::Background, move |_| async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let _ = internal.send(Internal::Transfer(TransferEvent::Finished {
            id,
            outcome: crate::transfer::TransferOutcome::Completed,
        }));
    });
    let mut engine = run_until_shutdown(t, Duration::from_millis(400)).await;

    let events = engine.shut_down().await;

    assert!(!events.iter().any(|event| matches!(event, Event::LocationChanged { .. })), "{events:?}");
    let entries = History::load(&engine.paths).0.entries().to_vec();
    assert_eq!(entries[0].result, HistoryResult::Interrupted);
}

#[tokio::test]
async fn dropping_an_engine_aborts_the_tasks_it_still_owns() {
    let t = test_engine();
    t.engine.tasks.spawn("stubborn", Scope::Background, |_| async move {
        std::future::pending::<()>().await;
    });
    let tasks = t.engine.tasks.clone();

    drop(t);

    assert!(tasks.live_names().is_empty());
}

#[tokio::test]
async fn a_transfer_task_that_panics_fails_its_job() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    t.engine.transfers.get_mut(id).unwrap().attempts = 3;
    t.engine.tasks.spawn("transfer", Scope::Transfer(id), |_| async move {
        panic!("transfer exploded");
    });

    t.run_internal().await;

    assert!(matches!(t.engine.transfers.get(id).unwrap().status, JobStatus::Failed(_)));
    assert!(!t.engine.tasks.take_cancelled(Scope::Transfer(id)));
}

#[tokio::test]
async fn a_planning_task_that_panics_clears_its_scan() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch_id = t.engine.transfers.start_batch("scan".to_string());
    t.engine.planning.push(PlanningScan {
        batch_id,
        session_id: session,
        direction: Direction::Upload,
        display_name: "scan".to_string(),
    });
    t.engine.tasks.spawn("plan-copy", Scope::Planning(batch_id), |_| async move {
        panic!("scan exploded");
    });

    t.run_internal().await;

    assert!(t.engine.planning.is_empty());
}

#[tokio::test]
async fn a_task_that_panics_after_its_job_already_finished_changes_nothing() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::Completed;
    t.engine.tasks.spawn("transfer", Scope::Transfer(id), |_| async move {
        panic!("too late");
    });

    t.run_internal().await;

    assert_eq!(t.engine.transfers.get(id).unwrap().status, JobStatus::Completed);
}

#[tokio::test]
async fn an_edit_upload_in_flight_is_waited_for_beyond_the_normal_grace() {
    let t = test_engine();
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let marker = completed.clone();
    t.engine.tasks.spawn("edit-upload", Scope::Edit(1), move |_| async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        marker.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let mut engine = run_until_shutdown(t, Duration::from_millis(50)).await;

    engine.shut_down().await;

    assert!(completed.load(std::sync::atomic::Ordering::Relaxed));
}

#[tokio::test]
async fn an_edit_upload_that_outlasts_the_upload_grace_is_aborted() {
    let t = test_engine();
    t.engine.tasks.spawn("edit-upload", Scope::Edit(1), move |_| async move {
        std::future::pending::<()>().await;
    });
    let tasks = t.engine.tasks.clone();
    let mut engine =
        run_until_shutdown_waiting_for_uploads(t, Duration::from_millis(50), Duration::from_millis(100)).await;
    let start = std::time::Instant::now();

    engine.shut_down().await;

    assert!(tasks.live_names().is_empty());
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn shutdown_without_an_upload_does_not_wait_for_the_upload_grace() {
    let t = test_engine();
    let mut engine = run_until_shutdown(t, Duration::from_millis(50)).await;
    let start = std::time::Instant::now();

    engine.shut_down().await;

    assert!(start.elapsed() < Duration::from_millis(1500));
}

#[tokio::test]
async fn a_history_write_that_never_finishes_does_not_block_the_shutdown() {
    use std::os::unix::ffi::OsStrExt;

    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    std::fs::create_dir_all(&t.engine.paths.state_dir).unwrap();
    let fifo = t.engine.paths.history_file().with_extension("toml.tmp");
    let c_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let mut engine = run_until_shutdown(t, Duration::from_millis(50)).await;
    let start = std::time::Instant::now();

    engine.shut_down().await;

    assert!(start.elapsed() < Duration::from_secs(5));
    let _release = std::fs::File::open(&fifo).unwrap();
}

#[test]
fn a_failed_connect_is_in_the_log_as_well_as_shown() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_internal(Internal::ConnectFailed { name: "prod".to_string(), message: "refused".to_string() });
    });

    assert!(logs.contains("ERROR") && logs.contains("refused"), "{logs}");
    assert!(t.drain().iter().any(|event| matches!(event, Event::ConnectFailed { .. })));
}

#[test]
fn a_warning_from_a_handler_is_in_the_log_too_and_shown_once() {
    let mut t = test_engine();
    t.engine.state_protected = true;

    let logs = crate::engine::testing::capture_logs(|| t.engine.remember_save_choice(true));

    assert!(logs.contains("WARN") && logs.contains("Couldn't remember the keyring choice"), "{logs}");
    let notices = t.notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].0, Severity::Warning);
}

#[test]
fn a_failed_transfer_is_in_the_log_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    t.engine.transfers.get_mut(id).unwrap().attempts = 3;

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_transfer_event(TransferEvent::Failed {
            id,
            message: "Transfer failed: a \u{2014} disk full".to_string(),
        });
        t.engine.process_row_changes();
    });

    assert_eq!(error_lines(&logs), 1, "{logs}");
    assert!(
        t.notices().iter().any(|(severity, message)| *severity == Severity::Error && message.contains("disk full"))
    );
}

#[test]
fn a_failed_scan_is_in_the_log_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch_id = t.engine.transfers.start_batch("scan".to_string());
    t.engine.planning.push(PlanningScan {
        batch_id,
        session_id: session,
        direction: Direction::Upload,
        display_name: "scan".to_string(),
    });

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_transfer_event(TransferEvent::PlanFailed {
            batch_id,
            message: "Copy failed: no access".to_string(),
        });
    });

    assert_eq!(error_lines(&logs), 1, "{logs}");
    assert!(t.notices().iter().any(|(_, message)| message.contains("no access")));
}

#[test]
fn a_panicking_transfer_leaves_the_panic_and_the_failed_job_in_the_log_and_nothing_else() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = queue_job(&mut t, session, "a");
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    t.engine.transfers.get_mut(id).unwrap().attempts = 3;

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.recover_from_panic("transfer", Scope::Transfer(id), "it exploded");
        t.engine.process_row_changes();
    });

    assert_eq!(error_lines(&logs), 2, "{logs}");
    assert!(logs.contains("it exploded"), "{logs}");
}

#[test]
fn a_panicking_background_task_is_in_the_log_once() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.recover_from_panic("persist-writer", Scope::Background, "it exploded");
    });

    assert_eq!(error_lines(&logs), 1, "{logs}");
    assert!(t.notices().iter().any(|(_, message)| message == "A background task failed: persist-writer"));
}

#[test]
fn a_cancelled_connection_is_not_logged_as_an_error() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_internal(Internal::ConnectFailed {
            name: "prod".to_string(),
            message: crate::CONNECTION_CANCELLED.to_string(),
        });
    });

    assert_eq!(error_lines(&logs), 0, "{logs}");
    assert!(logs.contains("INFO") && logs.contains("Connection cancelled"), "{logs}");
    assert!(t.drain().iter().any(|event| matches!(event, Event::ConnectFailed { .. })));
}

#[test]
fn a_rejected_profile_is_logged_as_a_warning_not_an_error() {
    let t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.profile_rejected("Name can't be empty");
    });

    assert_eq!(error_lines(&logs), 0, "{logs}");
    assert!(logs.contains("WARN") && logs.contains("Name can't be empty"), "{logs}");
}

#[test]
fn a_reported_problem_says_which_line_of_code_raised_it() {
    let t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| t.engine.report(Severity::Warning, "look here"));

    assert!(logs.contains("engine/tests.rs"), "{logs}");
}

#[test]
fn the_announce_helper_shows_a_notice_without_logging() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| t.engine.announce(Severity::Error, "logged elsewhere"));

    assert!(logs.is_empty(), "{logs}");
    assert_eq!(t.notices(), vec![(Severity::Error, "logged elsewhere".to_string())]);
}

#[test]
fn a_failure_for_a_scan_that_is_no_longer_tracked_is_still_logged() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_transfer_event(TransferEvent::PlanFailed {
            batch_id: 99,
            message: "Copy failed: gone".to_string(),
        });
    });

    assert_eq!(error_lines(&logs), 1, "{logs}");
    assert!(t.notices().iter().any(|(_, message)| message.contains("gone")));
}

#[test]
fn a_failure_for_a_job_that_no_longer_exists_is_still_logged() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_transfer_event(TransferEvent::Failed {
            id: 4242,
            message: "Transfer failed: vanished".to_string(),
        });
    });

    assert_eq!(error_lines(&logs), 1, "{logs}");
    assert!(t.notices().iter().any(|(_, message)| message.contains("vanished")));
}

#[test]
fn a_rejected_profile_is_logged_where_it_was_rejected() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_command(Command::SaveProfile {
            original: None,
            draft: Box::new(crate::profiles::ProfileDraft { protocol: "nothing".to_string(), ..Default::default() }),
        });
    });

    assert!(logs.contains("WARN") && logs.contains("protocol is available"), "{logs}");
    assert!(logs.contains("handlers/profiles"), "{logs}");
    assert!(!logs.contains("engine/mod.rs"), "{logs}");
}

#[test]
fn a_connection_failure_is_logged_where_it_was_raised() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| {
        t.engine.handle_command(Command::Connect { profile: "nobody".to_string() });
    });

    assert!(logs.contains("ERROR") && logs.contains("No saved connection named"), "{logs}");
    assert!(logs.contains("handlers/connect"), "{logs}");
}

#[test]
fn an_info_message_is_shown_without_a_problem_in_the_log() {
    let mut t = test_engine();

    let logs = crate::engine::testing::capture_logs(|| t.engine.info("all good"));

    assert!(!logs.contains("ERROR") && !logs.contains("WARN"), "{logs}");
    assert_eq!(t.notices(), vec![(Severity::Info, "all good".to_string())]);
}

fn error_lines(logs: &str) -> usize {
    logs.lines().filter(|line| line.contains("ERROR")).count()
}

#[test]
fn time_is_read_from_the_engine_clock_only() {
    fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, offenders);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if !name.ends_with(".rs") || name == "tests.rs" || name == "testing.rs" || path.ends_with("engine/mod.rs") {
                continue;
            }
            if std::fs::read_to_string(&path).unwrap().contains("Utc::now") {
                offenders.push(path.display().to_string());
            }
        }
    }

    let engine = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/engine");
    let mut offenders = Vec::new();
    visit(&engine, &mut offenders);

    assert!(offenders.is_empty(), "read the clock directly: {offenders:?}");
}
