use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};

use super::*;
use crate::{
    engine::{
        Command, PlanningScan, TransferEvent,
        testing::{TestEngine, test_engine},
    },
    history::{History, HistoryEntry, HistoryResult, testing::sample},
    transfer::{Direction, JobStatus},
};

fn last_history_event(t: &mut TestEngine) -> Vec<HistoryEntry> {
    t.drain()
        .into_iter()
        .rev()
        .find_map(|event| match event {
            Event::History(entries) => Some(entries),
            _ => None,
        })
        .expect("a history event")
}

#[test]
fn listing_history_replies_newest_first() {
    let mut t = test_engine();
    t.engine.history.store.record(sample("a", HistoryResult::Done)).unwrap();
    t.engine.history.store.record(sample("b", HistoryResult::Failed)).unwrap();
    t.drain();

    t.engine.handle_command(Command::ListHistory);

    let labels: Vec<String> = last_history_event(&mut t).into_iter().map(|entry| entry.label).collect();
    assert_eq!(labels, ["b", "a"]);
}

#[test]
fn listing_an_empty_history_replies_with_an_empty_list() {
    let mut t = test_engine();

    t.engine.handle_command(Command::ListHistory);

    assert!(last_history_event(&mut t).is_empty());
}

#[test]
fn clearing_history_empties_it_on_disk_and_replies_with_an_empty_list() {
    let mut t = test_engine();
    t.engine.history.store.record(sample("a", HistoryResult::Done)).unwrap();
    t.drain();

    t.engine.handle_command(Command::ClearHistory);

    assert!(last_history_event(&mut t).is_empty());
    assert!(History::load(&t.engine.paths).0.entries().is_empty());
}

#[test]
fn a_failed_clear_tells_the_user() {
    let mut t = test_engine();
    std::fs::write(&t.engine.paths.state_dir, b"a file where the directory should be").unwrap();

    t.engine.handle_command(Command::ClearHistory);

    let notice = t.first_notice().expect("a notice");
    assert_eq!(notice.0, Severity::Warning);
    assert!(notice.1.starts_with("Couldn't clear transfer history"), "{}", notice.1);
}

fn job(t: &mut TestEngine, session_id: u64, name: &str, batch_id: Option<u64>, status: JobStatus) -> u64 {
    let id = t.engine.transfers.enqueue(
        session_id,
        Direction::Upload,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        100,
        batch_id,
    );
    t.engine.transfers.get_mut(id).unwrap().status = status;
    id
}

fn recorded(t: &TestEngine) -> Vec<HistoryEntry> {
    t.engine.history.store.entries().to_vec()
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;

    fn make_writer(&'a self) -> LogBuffer {
        self.clone()
    }
}

fn capture(run: impl FnOnce()) -> String {
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buffer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let bytes = buffer.0.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn a_finished_single_upload_is_recorded_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "backup.tar", None, JobStatus::Completed);

    t.engine.publish_transfers();
    t.engine.publish_transfers();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.connection, "prod");
    assert_eq!(entry.direction, Direction::Upload);
    assert_eq!(entry.label, "backup.tar");
    assert_eq!(entry.local_path, "/local/backup.tar");
    assert_eq!(entry.remote_path, "/remote/backup.tar");
    assert_eq!((entry.files_done, entry.files_total, entry.bytes), (1, 1, 100));
    assert_eq!(entry.result, HistoryResult::Done);
    assert!(entry.failed_files.is_empty());
}

#[test]
fn a_finished_batch_records_the_common_directory_of_its_files() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("up".to_string());
    job(&mut t, session, "up/a", Some(batch), JobStatus::Completed);
    job(&mut t, session, "up/sub/b", Some(batch), JobStatus::Completed);

    t.engine.publish_transfers();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].label, "up");
    assert_eq!(entries[0].local_path, "/local/up");
    assert_eq!(entries[0].remote_path, "/remote/up");
    assert_eq!((entries[0].files_done, entries[0].files_total, entries[0].bytes), (2, 2, 200));
}

#[test]
fn a_batch_with_failures_lists_them() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("docs".to_string());
    job(&mut t, session, "a", Some(batch), JobStatus::Completed);
    job(&mut t, session, "b", Some(batch), JobStatus::Failed("boom".to_string()));
    job(&mut t, session, "c", Some(batch), JobStatus::Failed("bang".to_string()));

    t.engine.publish_transfers();

    let entry = &recorded(&t)[0];
    assert_eq!(entry.result, HistoryResult::PartlyFailed { failed: 2 });
    assert_eq!(entry.failed_files, ["b", "c"]);
    assert_eq!(entry.failed_count, 2);
    assert_eq!((entry.files_done, entry.files_total), (1, 3));
    assert_eq!(entry.bytes, 100);
}

#[test]
fn a_row_where_everything_failed_is_failed() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Failed("boom".to_string()));

    t.engine.publish_transfers();

    let entry = &recorded(&t)[0];
    assert_eq!(entry.result, HistoryResult::Failed);
    assert_eq!(entry.failed_files, ["a"]);
}

#[test]
fn a_cancelled_row_is_recorded_as_cancelled_without_failed_names() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Cancelled);

    t.engine.publish_transfers();

    let entry = &recorded(&t)[0];
    assert_eq!(entry.result, HistoryResult::Cancelled);
    assert!(entry.failed_files.is_empty());
}

#[test]
fn a_retry_that_finishes_again_is_a_second_entry() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = job(&mut t, session, "a", None, JobStatus::Failed("boom".to_string()));
    t.engine.publish_transfers();
    assert_eq!(recorded(&t).len(), 1);

    t.engine.transfers.retry_jobs(&[id]);
    t.engine.publish_transfers();
    assert_eq!(recorded(&t).len(), 1);

    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::Completed;
    t.engine.publish_transfers();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].result, HistoryResult::Failed);
    assert_eq!(entries[1].result, HistoryResult::Done);
}

#[test]
fn running_and_queued_rows_are_not_recorded() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::InProgress);
    job(&mut t, session, "b", None, JobStatus::Queued);

    t.engine.publish_transfers();

    assert!(recorded(&t).is_empty());
}

#[test]
fn clearing_finished_rows_does_not_create_entries() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Completed);
    t.engine.publish_transfers();

    t.engine.clear_finished_rows();
    t.engine.publish_transfers();

    assert_eq!(recorded(&t).len(), 1);
}

#[test]
fn recording_publishes_the_history_newest_first() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Completed);
    t.engine.publish_transfers();
    t.drain();
    job(&mut t, session, "b", None, JobStatus::Completed);

    t.engine.publish_transfers();

    let labels: Vec<String> = last_history_event(&mut t).into_iter().map(|entry| entry.label).collect();
    assert_eq!(labels, ["b", "a"]);
}

#[test]
fn nothing_is_published_when_nothing_was_recorded() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::InProgress);

    t.engine.publish_transfers();

    assert!(!t.drain().iter().any(|event| matches!(event, Event::History(_))));
}

#[test]
fn a_file_name_that_is_not_utf8_is_recorded_lossily() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = job(&mut t, session, "a", None, JobStatus::Completed);
    t.engine.transfers.get_mut(id).unwrap().local_path = PathBuf::from(OsStr::from_bytes(b"/local/caf\xe9.txt"));

    t.engine.publish_transfers();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].local_path, "/local/caf\u{fffd}.txt");
    assert!(t.notices().is_empty());
    assert_eq!(History::load(&t.engine.paths).0.entries().len(), 1);
}

#[test]
fn a_huge_failure_list_is_capped_but_counted() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("many".to_string());
    for index in 0..250 {
        job(&mut t, session, &format!("f{index}"), Some(batch), JobStatus::Failed("gone".to_string()));
    }

    t.engine.publish_transfers();

    let entry = &recorded(&t)[0];
    assert_eq!(entry.result, HistoryResult::Failed);
    assert_eq!(entry.failed_count, 250);
    assert_eq!(entry.failed_files.len(), crate::history::MAX_FAILED_FILES);
    assert_eq!(entry.failed_files[0], "f0");
}

#[test]
fn a_failed_write_warns_once_and_keeps_the_entries_in_memory() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    std::fs::write(&t.engine.paths.state_dir, b"a file where the directory should be").unwrap();
    job(&mut t, session, "a", None, JobStatus::Completed);
    t.engine.publish_transfers();
    job(&mut t, session, "b", None, JobStatus::Completed);
    t.engine.publish_transfers();

    let warnings: Vec<String> = t
        .notices()
        .into_iter()
        .filter(|(severity, message)| *severity == Severity::Warning && message.starts_with("Couldn't save transfer"))
        .map(|(_, message)| message)
        .collect();

    assert_eq!(warnings.len(), 1);
    assert_eq!(recorded(&t).len(), 2);
}

#[test]
fn each_failed_job_logs_one_error_line_with_its_reason() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("docs".to_string());
    job(&mut t, session, "a", Some(batch), JobStatus::Completed);
    job(&mut t, session, "b", Some(batch), JobStatus::Failed("Transfer failed: b \u{2014} disk full".to_string()));
    job(&mut t, session, "c", Some(batch), JobStatus::Failed("Transfer failed: c \u{2014} timed out".to_string()));

    let logs = capture(|| t.engine.publish_transfers());

    assert_eq!(logs.matches("ERROR").count(), 2, "{logs}");
    assert!(logs.contains("porthmos::transfers"), "{logs}");
    assert!(logs.contains("connection=prod"), "{logs}");
    assert!(logs.contains("file=b"), "{logs}");
    assert!(logs.contains("disk full"), "{logs}");
    assert!(logs.contains("timed out"), "{logs}");
}

#[test]
fn finished_rows_without_failures_log_nothing() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Completed);
    job(&mut t, session, "b", None, JobStatus::Cancelled);

    let logs = capture(|| t.engine.publish_transfers());

    assert!(!logs.contains("ERROR"), "{logs}");
}

#[test]
fn failure_reasons_never_reach_the_history_file() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Failed("secret reason text".to_string()));

    t.engine.publish_transfers();

    let text = std::fs::read_to_string(t.engine.paths.history_file()).unwrap();
    assert!(!text.contains("secret reason text"), "{text}");
    assert!(text.contains("a"));
}

fn scan(batch_id: u64, session_id: u64, label: &str) -> PlanningScan {
    PlanningScan {
        batch_id,
        session_id,
        direction: Direction::Upload,
        display_name: label.to_string(),
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

#[test]
fn a_copy_whose_scan_failed_is_recorded_as_failed_and_logged() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("photos".to_string());
    t.engine.planning.push(scan(batch, session, "photos"));

    let logs = capture(|| {
        t.engine.handle_transfer_event(TransferEvent::PlanFailed {
            batch_id: batch,
            message: "Copy failed: permission denied".to_string(),
        })
    });

    let entries = recorded(&t);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].label, "photos");
    assert_eq!(entries[0].connection, "prod");
    assert_eq!(entries[0].direction, Direction::Upload);
    assert_eq!(entries[0].result, HistoryResult::Failed);
    assert_eq!((entries[0].files_done, entries[0].files_total, entries[0].bytes), (0, 0, 0));
    assert!(entries[0].failed_files.is_empty());
    assert!(logs.contains("permission denied"), "{logs}");
    assert!(logs.contains("connection=prod"), "{logs}");
    assert!(last_history_event(&mut t).iter().any(|entry| entry.label == "photos"));
}

#[test]
fn a_scan_the_user_cancelled_is_not_recorded() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("photos".to_string());
    t.engine.planning.push(scan(batch, session, "photos"));

    t.engine.handle_transfer_event(TransferEvent::PlanCancelled {
        batch_id: batch,
        session_id: session,
        direction: Direction::Upload,
    });

    assert!(recorded(&t).is_empty());
}

#[test]
fn unfinished_rows_are_recorded_as_interrupted_with_the_progress_they_had() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("docs".to_string());
    job(&mut t, session, "a", Some(batch), JobStatus::Completed);
    let running = job(&mut t, session, "b", Some(batch), JobStatus::InProgress);
    t.engine.transfers.get_mut(running).unwrap().transferred_bytes = 40;
    job(&mut t, session, "queued.txt", None, JobStatus::Queued);
    let scanning = t.engine.transfers.start_batch("photos".to_string());
    t.engine.planning.push(scan(scanning, session, "photos"));
    job(&mut t, session, "done.txt", None, JobStatus::Completed);
    t.engine.publish_transfers();
    assert_eq!(recorded(&t).len(), 1);

    t.engine.record_interrupted();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 4);
    let by_label = |label: &str| entries.iter().find(|entry| entry.label == label).unwrap().clone();
    let batch_entry = by_label("docs");
    assert_eq!(batch_entry.result, HistoryResult::Interrupted);
    assert_eq!((batch_entry.files_done, batch_entry.files_total, batch_entry.bytes), (1, 2, 140));
    assert_eq!(by_label("queued.txt").result, HistoryResult::Interrupted);
    let scan_entry = by_label("photos");
    assert_eq!(scan_entry.result, HistoryResult::Interrupted);
    assert_eq!(scan_entry.connection, "prod");
    assert_eq!((scan_entry.files_done, scan_entry.files_total), (0, 0));
    assert_eq!(by_label("done.txt").result, HistoryResult::Done);
}

#[test]
fn nothing_is_recorded_at_shutdown_when_every_row_already_finished() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Completed);
    t.engine.publish_transfers();

    t.engine.record_interrupted();

    assert_eq!(recorded(&t).len(), 1);
}

#[tokio::test]
async fn quitting_records_unfinished_rows_before_the_engine_stops() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::InProgress);
    let TestEngine { engine, internal, dir, .. } = t;
    let paths = engine.paths.clone();
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    commands.send(Command::Shutdown).unwrap();

    engine.run(receiver, internal).await;

    let entries = History::load(&paths).0.entries().to_vec();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].result, HistoryResult::Interrupted);
    assert_eq!(entries[0].label, "a");
    drop(dir);
}

#[test]
fn a_row_that_failed_because_of_a_disconnect_keeps_the_connection_name() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Queued);

    t.engine.handle_command(Command::Disconnect { session });

    let entries = recorded(&t);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].connection, "prod");
    assert_eq!(entries[0].result, HistoryResult::Failed);
    assert_eq!(entries[0].failed_files, ["a"]);
}

#[test]
fn a_row_that_finishes_after_the_disconnect_still_has_the_name() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = job(&mut t, session, "a", None, JobStatus::InProgress);
    t.engine.handle_command(Command::Disconnect { session });
    assert!(recorded(&t).is_empty());

    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::Cancelled;
    t.engine.publish_transfers();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].connection, "prod");
    assert_eq!(entries[0].result, HistoryResult::Cancelled);
}

#[test]
fn an_engine_dropped_before_it_ever_ran_still_records_unfinished_rows() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::InProgress);
    let TestEngine { engine, internal, dir, .. } = t;
    let paths = engine.paths.clone();
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    commands.send(Command::Shutdown).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.spawn(engine.run(receiver, internal));

    runtime.shutdown_timeout(Duration::from_millis(500));

    let entries = History::load(&paths).0.entries().to_vec();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].result, HistoryResult::Interrupted);
    drop(dir);
}

#[test]
fn recording_the_interrupted_rows_twice_records_each_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::InProgress);

    t.engine.record_interrupted();
    t.engine.record_interrupted();

    assert_eq!(recorded(&t).len(), 1);
}

#[test]
fn interrupted_rows_log_the_reasons_of_files_that_had_already_failed() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("docs".to_string());
    job(&mut t, session, "a", Some(batch), JobStatus::Failed("Transfer failed: a \u{2014} disk full".to_string()));
    job(&mut t, session, "b", Some(batch), JobStatus::InProgress);

    let logs = capture(|| t.engine.record_interrupted());

    assert!(logs.contains("disk full"), "{logs}");
    assert!(logs.contains("connection=prod"), "{logs}");
    assert_eq!(recorded(&t)[0].failed_files, ["a"]);
}

#[test]
fn a_one_file_copy_records_the_file_paths() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("a.txt".to_string());
    job(&mut t, session, "a.txt", Some(batch), JobStatus::Completed);

    t.engine.publish_transfers();

    let entry = &recorded(&t)[0];
    assert_eq!(entry.local_path, "/local/a.txt");
    assert_eq!(entry.remote_path, "/remote/a.txt");
}

#[test]
fn a_folder_holding_one_file_still_records_the_folder() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("up".to_string());
    job(&mut t, session, "up/a", Some(batch), JobStatus::Completed);

    t.engine.publish_transfers();

    let entry = &recorded(&t)[0];
    assert_eq!(entry.local_path, "/local/up");
    assert_eq!(entry.remote_path, "/remote/up");
}

#[test]
fn a_retry_entry_counts_only_what_that_attempt_did() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("docs".to_string());
    job(&mut t, session, "a", Some(batch), JobStatus::Completed);
    let failed = job(&mut t, session, "b", Some(batch), JobStatus::Failed("boom".to_string()));
    t.engine.publish_transfers();
    t.engine.transfers.retry_jobs(&[failed]);
    t.engine.publish_transfers();

    t.engine.transfers.get_mut(failed).unwrap().status = JobStatus::Completed;
    t.engine.publish_transfers();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 2);
    assert_eq!((entries[0].files_done, entries[0].files_total, entries[0].bytes), (1, 2, 100));
    assert_eq!(entries[1].result, HistoryResult::Done);
    assert_eq!((entries[1].files_done, entries[1].files_total, entries[1].bytes), (1, 1, 100));
    assert!(entries[1].failed_files.is_empty());
}

#[test]
fn a_finished_row_is_recorded_when_its_change_is_processed_without_building_a_snapshot() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Completed);

    t.engine.process_row_changes();

    assert_eq!(recorded(&t).len(), 1);
    assert!(!t.drain().iter().any(|event| matches!(event, Event::TransfersChanged(_))));
}

#[test]
fn processing_twice_records_a_row_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "a", None, JobStatus::Completed);

    t.engine.process_row_changes();
    t.engine.process_row_changes();

    assert_eq!(recorded(&t).len(), 1);
}

#[test]
fn a_finish_reopen_finish_burst_inside_one_drain_is_recorded_once() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let batch = t.engine.transfers.start_batch("docs".to_string());
    job(&mut t, session, "a", Some(batch), JobStatus::Completed);
    job(&mut t, session, "b", Some(batch), JobStatus::Failed("boom".to_string()));

    t.engine.process_row_changes();

    let entries = recorded(&t);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].result, HistoryResult::PartlyFailed { failed: 1 });
}

#[test]
fn a_row_that_reopens_before_the_drain_is_not_recorded_until_it_finishes() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = job(&mut t, session, "a", None, JobStatus::Failed("boom".to_string()));
    t.engine.transfers.retry_jobs(&[id]);

    t.engine.process_row_changes();

    assert!(recorded(&t).is_empty());
    t.engine.transfers.get_mut(id).unwrap().status = JobStatus::Completed;
    t.engine.process_row_changes();
    assert_eq!(recorded(&t).len(), 1);
}

#[test]
fn a_row_cleared_before_the_drain_leaves_no_entry_and_no_stale_marks() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    let id = job(&mut t, session, "a", None, JobStatus::Completed);
    t.engine.transfers.remove_jobs(&[id]);

    t.engine.process_row_changes();

    assert!(recorded(&t).is_empty());
    assert!(t.engine.history.recorded.is_empty());
    assert!(t.engine.history.counted.is_empty());
}

#[test]
fn unprocessed_finished_rows_are_recorded_before_the_interrupted_ones() {
    let mut t = test_engine();
    let (session, _fs) = t.add_session("prod");
    job(&mut t, session, "done.txt", None, JobStatus::Completed);
    job(&mut t, session, "running.txt", None, JobStatus::InProgress);

    t.engine.record_interrupted();

    let results: Vec<(String, HistoryResult)> =
        recorded(&t).into_iter().map(|entry| (entry.label, entry.result)).collect();
    assert_eq!(
        results,
        vec![("done.txt".to_string(), HistoryResult::Done), ("running.txt".to_string(), HistoryResult::Interrupted)]
    );
}

#[test]
fn finishing_a_huge_batch_costs_the_same_per_event_as_a_small_one() {
    fn total_time(jobs: usize) -> f64 {
        let mut t = test_engine();
        let (session, _fs) = t.add_session("prod");
        let batch = t.engine.transfers.start_batch("big".to_string());
        let ids: Vec<u64> = (0..jobs)
            .map(|index| {
                t.engine.transfers.enqueue(
                    session,
                    Direction::Upload,
                    PathBuf::from(format!("/local/f{index}")),
                    format!("/remote/f{index}"),
                    format!("f{index}"),
                    10,
                    Some(batch),
                )
            })
            .collect();
        let start = std::time::Instant::now();
        for id in &ids {
            t.engine.transfers.get_mut(*id).unwrap().status = JobStatus::Completed;
            t.engine.process_row_changes();
        }
        start.elapsed().as_secs_f64() / jobs as f64
    }

    let small = (0..3).map(|_| total_time(1_000)).fold(f64::MAX, f64::min);
    let large = (0..3).map(|_| total_time(50_000)).fold(f64::MAX, f64::min);

    assert!(large < small * 30.0, "per event: {small:.9}s for 1k jobs, {large:.9}s for 50k jobs");
}
