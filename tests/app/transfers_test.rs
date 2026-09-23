use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::*;
use crate::{
    connection::ConnectionSource,
    transfer::plan::{DirectoryPlan, PlannedFile},
};

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

fn app_with_a_local_directory_selected() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("myfolder")).unwrap();
    let mut app = App::at(dir.path().to_path_buf()).unwrap();
    app.local.cursor = app.local.rows().len() - 1;
    (dir, app)
}

fn sample_connection_entry() -> ConnectionEntry {
    ConnectionEntry { name: "test".to_string(), host: "test.example.com".to_string(), port: 22, username: "user".to_string(), identity_file: None, remote_path: None, password: None, source: ConnectionSource::Profile }
}

fn planning_scan(batch_id: u64, name: &str) -> PlanningScan {
    PlanningScan { batch_id, session_id: 1, direction: Direction::Upload, display_name: name.to_string(), cancel: Arc::new(AtomicBool::new(false)) }
}

#[test]
fn fill_transfer_slots_notifies_when_the_jobs_session_has_disconnected() {
    let (_dir, mut app) = app_in_temp_dir();
    app.transfers.enqueue(999, Direction::Upload, PathBuf::from("/local/file.txt"), "/remote/file.txt".to_string(), "file.txt".to_string(), 100, None);

    app.fill_transfer_slots();

    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Error);
    assert!(notification.message.contains("file.txt"));
    assert!(notification.message.contains("disconnected"));
}

#[test]
fn copying_a_directory_with_a_disconnected_session_fails_without_spawning() {
    let (_dir, mut app) = app_with_a_local_directory_selected();
    app.sessions.insert(sample_connection_entry(), PanelState::from_listing(PathBuf::from("/remote"), Vec::new()));

    app.start_copy();

    assert!(app.planning.is_empty());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Error);
    assert!(notification.message.contains("disconnected"));
    assert_eq!(app.transfers.queued_count(), 0);
}

#[test]
fn plan_ready_enqueues_every_planned_file_under_the_batch_id() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch("batch".to_string());
    let plan = DirectoryPlan { files: vec![PlannedFile { local_path: PathBuf::from("/local/a.txt"), remote_path: "/remote/a.txt".to_string(), display_name: "a.txt".to_string(), size: 10, existing: None, source_modified: None }, PlannedFile { local_path: PathBuf::from("/local/b.txt"), remote_path: "/remote/b.txt".to_string(), display_name: "b.txt".to_string(), size: 20, existing: None, source_modified: None }], skipped_symlinks: 0, taken_names: HashMap::new() };

    app.apply_plan_ready(batch_id, 1, Direction::Upload, plan, &[]);

    let progress = app.transfers.batch_progress(batch_id);
    assert_eq!(progress.total_files, 2);
    assert_eq!(progress.total_bytes, 30);
}

#[test]
fn plan_ready_warns_once_about_skipped_symlinks() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch("batch".to_string());
    let plan = DirectoryPlan { files: Vec::new(), skipped_symlinks: 3, taken_names: HashMap::new() };

    app.apply_plan_ready(batch_id, 1, Direction::Upload, plan, &[]);

    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Warning);
    assert!(notification.message.contains("3 symlinks"));
}

#[test]
fn plan_ready_fails_without_enqueueing_when_the_session_has_disconnected() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch("batch".to_string());
    app.planning.push(planning_scan(batch_id, "myfolder"));
    let plan = DirectoryPlan { files: vec![PlannedFile { local_path: PathBuf::from("/local/a.txt"), remote_path: "/remote/a.txt".to_string(), display_name: "a.txt".to_string(), size: 10, existing: None, source_modified: None }], skipped_symlinks: 0, taken_names: HashMap::new() };

    app.apply_transfer_event(TransferEvent::PlanReady { batch_id, session_id: 1, direction: Direction::Upload, plan });

    assert!(app.planning.is_empty());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Error);
    assert!(notification.message.contains("disconnected"));
    assert_eq!(app.transfers.batch_progress(batch_id).total_files, 0);
}

#[test]
fn plan_failed_clears_planning_and_shows_an_error() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch("batch".to_string());
    app.planning.push(planning_scan(batch_id, "myfolder"));

    app.apply_transfer_event(TransferEvent::PlanFailed { batch_id, message: "Copy failed: permission denied".to_string() });

    assert!(app.planning.is_empty());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Error);
    assert_eq!(notification.message, "Copy failed: permission denied");
}

#[test]
fn plan_cancelled_clears_planning_and_shows_an_info_notification() {
    let (_dir, mut app) = app_in_temp_dir();
    let session_id = app.sessions.insert(sample_connection_entry(), PanelState::from_listing(PathBuf::from("/remote"), Vec::new()));
    let batch_id = app.transfers.start_batch("batch".to_string());
    app.planning.push(planning_scan(batch_id, "myfolder"));

    app.apply_transfer_event(TransferEvent::PlanCancelled { batch_id, session_id, direction: Direction::Upload });

    assert!(app.planning.is_empty());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Info);
    assert_eq!(notification.message, "Copy cancelled");
    assert_eq!(app.transfers.batch_progress(batch_id).total_files, 0);
}

#[test]
fn plan_cancelled_only_clears_its_own_scan() {
    let (_dir, mut app) = app_in_temp_dir();
    let first = app.transfers.start_batch("batch".to_string());
    let second = app.transfers.start_batch("batch".to_string());
    app.planning.push(planning_scan(first, "one"));
    app.planning.push(planning_scan(second, "two"));

    app.apply_transfer_event(TransferEvent::PlanCancelled { batch_id: first, session_id: 1, direction: Direction::Upload });

    let remaining: Vec<u64> = app.planning.iter().map(|scan| scan.batch_id).collect();
    assert_eq!(remaining, vec![second]);
}

#[test]
fn cancel_all_copies_sets_every_scans_flag_and_leaves_them_tracked() {
    let (_dir, mut app) = app_in_temp_dir();
    app.planning.push(planning_scan(0, "one"));
    app.planning.push(planning_scan(1, "two"));

    app.cancel_all_copies();

    assert!(app.planning.iter().all(|scan| scan.cancel.load(Ordering::Relaxed)));
    assert_eq!(app.planning.len(), 2);
}

#[test]
fn cancel_all_copies_stops_scans_and_the_active_batch_together() {
    let (_dir, mut app) = app_in_temp_dir();
    app.planning.push(planning_scan(100, "scanning"));
    let batch_id = app.transfers.start_batch("batch".to_string());
    let active = app.transfers.enqueue(1, Direction::Upload, PathBuf::from("/local/a.txt"), "/remote/a.txt".to_string(), "a.txt".to_string(), 10, Some(batch_id));
    let queued = app.transfers.enqueue(1, Direction::Upload, PathBuf::from("/local/b.txt"), "/remote/b.txt".to_string(), "b.txt".to_string(), 10, Some(batch_id));
    app.transfers.get_mut(active).unwrap().status = JobStatus::InProgress;
    let transfer_cancel = Arc::new(AtomicBool::new(false));
    app.transfer_cancels.insert(active, transfer_cancel.clone());

    app.cancel_all_copies();

    assert!(app.planning[0].cancel.load(Ordering::Relaxed));
    assert!(transfer_cancel.load(Ordering::Relaxed));
    assert_eq!(app.transfers.get(queued).unwrap().status, JobStatus::Cancelled);
}

#[test]
fn cancel_all_copies_with_nothing_running_is_a_no_op() {
    let (_dir, mut app) = app_in_temp_dir();

    app.cancel_all_copies();

    assert!(app.planning.is_empty());
    assert!(app.notifications.current().is_none());
}

fn enqueue_job(app: &mut App, session_id: u64, name: &str, batch_id: Option<u64>) -> u64 {
    app.transfers.enqueue(session_id, Direction::Upload, PathBuf::from(format!("/local/{name}")), format!("/remote/{name}"), name.to_string(), 10, batch_id)
}

fn mark_active(app: &mut App, id: u64) -> Arc<AtomicBool> {
    app.transfers.get_mut(id).unwrap().status = JobStatus::InProgress;
    let cancel = Arc::new(AtomicBool::new(false));
    app.transfer_cancels.insert(id, cancel.clone());
    cancel
}

#[test]
fn fill_transfer_slots_fails_every_job_whose_session_is_gone_without_starting_any() {
    let (_dir, mut app) = app_in_temp_dir();
    let jobs: Vec<u64> = (0..6).map(|n| enqueue_job(&mut app, 999, &format!("{n}.txt"), None)).collect();

    app.fill_transfer_slots();

    assert!(jobs.iter().all(|id| matches!(app.transfers.get(*id).unwrap().status, JobStatus::Failed(_))));
    assert!(app.transfer_cancels.is_empty());
    assert_eq!(app.transfers.active_count(), 0);
}

#[test]
fn cancel_all_copies_flags_every_active_job_and_cancels_every_queued_job() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch("batch".to_string());
    let first_active = enqueue_job(&mut app, 1, "a.txt", Some(batch_id));
    let second_active = enqueue_job(&mut app, 1, "b.txt", None);
    let queued_in_batch = enqueue_job(&mut app, 1, "c.txt", Some(batch_id));
    let queued_loose = enqueue_job(&mut app, 2, "d.txt", None);
    let first_cancel = mark_active(&mut app, first_active);
    let second_cancel = mark_active(&mut app, second_active);

    app.cancel_all_copies();

    assert!(first_cancel.load(Ordering::Relaxed));
    assert!(second_cancel.load(Ordering::Relaxed));
    assert_eq!(app.transfers.get(queued_in_batch).unwrap().status, JobStatus::Cancelled);
    assert_eq!(app.transfers.get(queued_loose).unwrap().status, JobStatus::Cancelled);
    assert_eq!(app.transfers.get(first_active).unwrap().status, JobStatus::InProgress);
}

#[test]
fn nothing_is_startable_after_cancel_all_copies() {
    let (_dir, mut app) = app_in_temp_dir();
    let active = enqueue_job(&mut app, 1, "a.txt", None);
    enqueue_job(&mut app, 1, "b.txt", None);
    mark_active(&mut app, active);

    app.cancel_all_copies();
    app.apply_transfer_event(TransferEvent::Finished { id: active, outcome: TransferOutcome::Cancelled });

    assert!(app.transfers.startable(app.max_parallel).is_empty());
    assert!(app.notifications.current().is_none());
}

#[test]
fn cancel_session_transfers_flags_only_that_sessions_jobs_and_scans() {
    let (_dir, mut app) = app_in_temp_dir();
    let mine = enqueue_job(&mut app, 1, "a.txt", None);
    let my_cancel = mark_active(&mut app, mine);
    app.planning.push(PlanningScan { batch_id: 7, session_id: 1, direction: Direction::Upload, display_name: "mine".to_string(), cancel: Arc::new(AtomicBool::new(false)) });

    let flagged = app.cancel_session_transfers(1);

    assert_eq!(flagged, 2);
    assert!(my_cancel.load(Ordering::Relaxed));
    assert!(app.planning[0].cancel.load(Ordering::Relaxed));
}

#[test]
fn cancel_session_transfers_leaves_other_sessions_alone() {
    let (_dir, mut app) = app_in_temp_dir();
    let theirs = enqueue_job(&mut app, 2, "a.txt", None);
    let their_cancel = mark_active(&mut app, theirs);
    app.planning.push(PlanningScan { batch_id: 7, session_id: 2, direction: Direction::Upload, display_name: "theirs".to_string(), cancel: Arc::new(AtomicBool::new(false)) });

    let flagged = app.cancel_session_transfers(1);

    assert_eq!(flagged, 0);
    assert!(!their_cancel.load(Ordering::Relaxed));
    assert!(!app.planning[0].cancel.load(Ordering::Relaxed));
}

#[test]
fn a_finished_event_removes_only_its_own_cancel_flag() {
    let (_dir, mut app) = app_in_temp_dir();
    let first = enqueue_job(&mut app, 1, "a.txt", None);
    let second = enqueue_job(&mut app, 1, "b.txt", None);
    mark_active(&mut app, first);
    mark_active(&mut app, second);

    app.apply_transfer_event(TransferEvent::Finished { id: first, outcome: TransferOutcome::Completed });

    assert!(!app.transfer_cancels.contains_key(&first));
    assert!(app.transfer_cancels.contains_key(&second));
}

#[test]
fn a_failed_event_removes_its_cancel_flag_and_requeues_within_the_retry_limit() {
    let (_dir, mut app) = app_in_temp_dir();
    let job = enqueue_job(&mut app, 999, "a.txt", None);
    mark_active(&mut app, job);
    app.transfers.get_mut(job).unwrap().attempts = 1;

    app.apply_transfer_event(TransferEvent::Failed { id: job, message: "Transfer failed: a.txt".to_string() });

    assert!(!app.transfer_cancels.contains_key(&job));
    assert!(matches!(app.transfers.get(job).unwrap().status, JobStatus::Failed(ref reason) if reason == "session disconnected"));
}

#[test]
fn a_cancelled_job_that_ends_in_an_error_is_not_retried() {
    let (_dir, mut app) = app_in_temp_dir();
    let job = enqueue_job(&mut app, 999, "a.txt", None);
    let cancel = mark_active(&mut app, job);
    cancel.store(true, Ordering::Relaxed);

    app.apply_transfer_event(TransferEvent::Failed { id: job, message: "Transfer failed: a.txt".to_string() });

    assert_eq!(app.transfers.get(job).unwrap().status, JobStatus::Cancelled);
    assert!(app.notifications.current().is_none());
}

#[test]
fn a_permanent_failure_of_the_last_pending_job_refreshes_the_destination() {
    let (dir, mut app) = app_in_temp_dir();
    let done = app.transfers.enqueue(1, Direction::Download, dir.path().join("done.txt"), "/remote/done.txt".to_string(), "done.txt".to_string(), 10, None);
    app.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    std::fs::write(dir.path().join("done.txt"), b"x").unwrap();
    let failing = app.transfers.enqueue(1, Direction::Download, dir.path().join("bad.txt"), "/remote/bad.txt".to_string(), "bad.txt".to_string(), 10, None);
    mark_active(&mut app, failing);
    app.transfers.get_mut(failing).unwrap().attempts = 3;

    app.apply_transfer_event(TransferEvent::Failed { id: failing, message: "Transfer failed: bad.txt".to_string() });

    assert!(app.local.rows().iter().any(|row| matches!(row, crate::tui::panels::Row::Entry(entry) if entry.name == "done.txt")));
}

#[test]
fn a_job_that_fails_to_start_refreshes_the_destination_once_nothing_is_pending() {
    let (dir, mut app) = app_in_temp_dir();
    let done = app.transfers.enqueue(999, Direction::Download, dir.path().join("done.txt"), "/remote/done.txt".to_string(), "done.txt".to_string(), 10, None);
    app.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    std::fs::write(dir.path().join("done.txt"), b"x").unwrap();
    app.transfers.enqueue(999, Direction::Download, dir.path().join("bad.txt"), "/remote/bad.txt".to_string(), "bad.txt".to_string(), 10, None);

    app.fill_transfer_slots();

    assert!(app.local.rows().iter().any(|row| matches!(row, crate::tui::panels::Row::Entry(entry) if entry.name == "done.txt")));
}

#[test]
fn plan_cancelled_for_a_disconnected_session_is_silent() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch("batch".to_string());
    app.planning.push(planning_scan(batch_id, "myfolder"));

    app.apply_transfer_event(TransferEvent::PlanCancelled { batch_id, session_id: 1, direction: Direction::Upload });

    assert!(app.planning.is_empty());
    assert!(app.notifications.current().is_none());
}

fn download_job(app: &mut App, dir: &std::path::Path, session_id: u64, name: &str) -> u64 {
    app.transfers.enqueue(session_id, Direction::Download, dir.join(name), format!("/remote/{name}"), name.to_string(), 10, None)
}

fn local_panel_shows(app: &App, name: &str) -> bool {
    app.local.rows().iter().any(|row| matches!(row, crate::tui::panels::Row::Entry(entry) if entry.name == name))
}

#[test]
fn a_finished_download_waits_to_refresh_while_another_download_of_that_session_runs() {
    let (dir, mut app) = app_in_temp_dir();
    let finished = download_job(&mut app, dir.path(), 1, "a.txt");
    let still_running = download_job(&mut app, dir.path(), 1, "b.txt");
    mark_active(&mut app, finished);
    mark_active(&mut app, still_running);
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();

    app.apply_transfer_event(TransferEvent::Finished { id: finished, outcome: TransferOutcome::Completed });

    assert!(!local_panel_shows(&app, "a.txt"));
}

#[test]
fn a_finished_download_refreshes_despite_other_sessions_and_upload_jobs() {
    let (dir, mut app) = app_in_temp_dir();
    let finished = download_job(&mut app, dir.path(), 1, "a.txt");
    mark_active(&mut app, finished);
    let other_session_download = download_job(&mut app, dir.path(), 2, "b.txt");
    mark_active(&mut app, other_session_download);
    let same_session_upload = enqueue_job(&mut app, 1, "c.txt", None);
    mark_active(&mut app, same_session_upload);
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();

    app.apply_transfer_event(TransferEvent::Finished { id: finished, outcome: TransferOutcome::Completed });

    assert!(local_panel_shows(&app, "a.txt"));
}

#[test]
fn cancel_all_copies_refreshes_destinations_left_with_nothing_pending() {
    let (dir, mut app) = app_in_temp_dir();
    let done = download_job(&mut app, dir.path(), 1, "done.txt");
    app.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    std::fs::write(dir.path().join("done.txt"), b"x").unwrap();
    download_job(&mut app, dir.path(), 1, "later.txt");

    app.cancel_all_copies();

    assert!(local_panel_shows(&app, "done.txt"));
}

#[test]
fn plan_failed_and_cancelled_forget_the_batch_label() {
    let (_dir, mut app) = app_in_temp_dir();
    let failed = app.transfers.start_batch("one".to_string());
    let cancelled = app.transfers.start_batch("two".to_string());

    app.apply_transfer_event(TransferEvent::PlanFailed { batch_id: failed, message: "Copy failed: boom".to_string() });
    app.apply_transfer_event(TransferEvent::PlanCancelled { batch_id: cancelled, session_id: 1, direction: Direction::Upload });

    assert_eq!(app.transfers.batch_label(failed), None);
    assert_eq!(app.transfers.batch_label(cancelled), None);
}

#[test]
fn plan_ready_for_a_disconnected_session_forgets_the_batch_label() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch("photos".to_string());
    let plan = DirectoryPlan { files: Vec::new(), skipped_symlinks: 0, taken_names: HashMap::new() };

    app.apply_transfer_event(TransferEvent::PlanReady { batch_id, session_id: 1, direction: Direction::Upload, plan });

    assert_eq!(app.transfers.batch_label(batch_id), None);
}

#[test]
fn a_ready_plan_keeps_its_label_only_when_it_has_files() {
    let (_dir, mut app) = app_in_temp_dir();
    let with_files = app.transfers.start_batch("photos".to_string());
    let empty = app.transfers.start_batch("empty".to_string());
    let file = PlannedFile { local_path: PathBuf::from("/local/a.txt"), remote_path: "/remote/a.txt".to_string(), display_name: "a.txt".to_string(), size: 10, existing: None, source_modified: None };

    app.apply_plan_ready(with_files, 1, Direction::Upload, DirectoryPlan { files: vec![file], skipped_symlinks: 0, taken_names: HashMap::new() }, &[]);
    app.apply_plan_ready(empty, 1, Direction::Upload, DirectoryPlan { files: Vec::new(), skipped_symlinks: 0, taken_names: HashMap::new() }, &[]);

    assert_eq!(app.transfers.batch_label(with_files), Some("photos"));
    assert_eq!(app.transfers.batch_label(empty), None);
}

#[test]
fn drain_pending_transfer_events_applies_every_queued_event() {
    let (_dir, mut app) = app_in_temp_dir();
    let job = enqueue_job(&mut app, 1, "a.txt", None);
    mark_active(&mut app, job);
    app.transfer_tx.send(TransferEvent::Progress { id: job, transferred: 5 }).unwrap();
    app.transfer_tx.send(TransferEvent::Progress { id: job, transferred: 10 }).unwrap();
    app.transfer_tx.send(TransferEvent::Finished { id: job, outcome: TransferOutcome::Completed }).unwrap();

    app.drain_pending_transfer_events();

    let finished = app.transfers.get(job).unwrap();
    assert_eq!((finished.status.clone(), finished.transferred_bytes), (JobStatus::Completed, 10));
    assert!(app.transfer_rx.try_recv().is_err());
}
