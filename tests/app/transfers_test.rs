use super::*;
use crate::connection::ConnectionSource;
use crate::transfer::plan::{DirectoryPlan, PlannedFile};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    ConnectionEntry {
        name: "test".to_string(),
        host: "test.example.com".to_string(),
        port: 22,
        username: "user".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }
}

fn planning_scan(batch_id: u64, name: &str) -> PlanningScan {
    PlanningScan { batch_id, display_name: name.to_string(), cancel: Arc::new(AtomicBool::new(false)) }
}

#[test]
fn maybe_start_next_transfer_notifies_when_the_jobs_session_has_disconnected() {
    let (_dir, mut app) = app_in_temp_dir();
    app.transfers.enqueue(
        999,
        Direction::Upload,
        PathBuf::from("/local/file.txt"),
        "/remote/file.txt".to_string(),
        "file.txt".to_string(),
        100,
        None,
    );

    app.maybe_start_next_transfer();

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
    assert!(app.transfers.next_to_run().is_none());
}

#[test]
fn plan_ready_enqueues_every_planned_file_under_the_batch_id() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    let plan = DirectoryPlan {
        files: vec![
            PlannedFile {
                local_path: PathBuf::from("/local/a.txt"),
                remote_path: "/remote/a.txt".to_string(),
                display_name: "a.txt".to_string(),
                size: 10,
            },
            PlannedFile {
                local_path: PathBuf::from("/local/b.txt"),
                remote_path: "/remote/b.txt".to_string(),
                display_name: "b.txt".to_string(),
                size: 20,
            },
        ],
        skipped_symlinks: 0,
    };

    app.apply_plan_ready(batch_id, 1, Direction::Upload, plan);

    let progress = app.transfers.batch_progress(batch_id);
    assert_eq!(progress.total_files, 2);
    assert_eq!(progress.total_bytes, 30);
}

#[test]
fn plan_ready_warns_once_about_skipped_symlinks() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    let plan = DirectoryPlan { files: Vec::new(), skipped_symlinks: 3 };

    app.apply_plan_ready(batch_id, 1, Direction::Upload, plan);

    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Warning);
    assert!(notification.message.contains("3 symlinks"));
}

#[test]
fn plan_ready_fails_without_enqueueing_when_the_session_has_disconnected() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    app.planning.push(planning_scan(batch_id, "myfolder"));
    let plan = DirectoryPlan {
        files: vec![PlannedFile {
            local_path: PathBuf::from("/local/a.txt"),
            remote_path: "/remote/a.txt".to_string(),
            display_name: "a.txt".to_string(),
            size: 10,
        }],
        skipped_symlinks: 0,
    };

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
    let batch_id = app.transfers.start_batch();
    app.planning.push(planning_scan(batch_id, "myfolder"));

    app.apply_transfer_event(TransferEvent::PlanFailed {
        batch_id,
        message: "Copy failed: permission denied".to_string(),
    });

    assert!(app.planning.is_empty());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Error);
    assert_eq!(notification.message, "Copy failed: permission denied");
}

#[test]
fn cancel_active_transfer_cancels_every_other_queued_job_in_the_same_batch() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    let active = app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        10,
        Some(batch_id),
    );
    let queued = app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/b.txt"),
        "/remote/b.txt".to_string(),
        "b.txt".to_string(),
        10,
        Some(batch_id),
    );
    app.transfers.get_mut(active).unwrap().status = JobStatus::InProgress;
    let cancel = Arc::new(AtomicBool::new(false));
    app.active_transfer_cancel = Some(cancel.clone());

    app.cancel_active_transfer();

    assert!(cancel.load(Ordering::Relaxed));
    assert_eq!(app.transfers.get(queued).unwrap().status, JobStatus::Cancelled);
    assert_eq!(app.transfers.get(active).unwrap().status, JobStatus::InProgress);
}

#[test]
fn cancel_active_transfer_is_a_plain_cancel_for_a_non_batch_job() {
    let (_dir, mut app) = app_in_temp_dir();
    let active = app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        10,
        None,
    );
    app.transfers.get_mut(active).unwrap().status = JobStatus::InProgress;
    let cancel = Arc::new(AtomicBool::new(false));
    app.active_transfer_cancel = Some(cancel.clone());

    app.cancel_active_transfer();

    assert!(cancel.load(Ordering::Relaxed));
}

#[test]
fn plan_cancelled_clears_planning_and_shows_an_info_notification() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    app.planning.push(planning_scan(batch_id, "myfolder"));

    app.apply_transfer_event(TransferEvent::PlanCancelled { batch_id, session_id: 1, direction: Direction::Upload });

    assert!(app.planning.is_empty());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Info);
    assert_eq!(notification.message, "Copy cancelled");
    assert_eq!(app.transfers.batch_progress(batch_id).total_files, 0);
}

#[test]
fn plan_cancelled_only_clears_its_own_scan() {
    let (_dir, mut app) = app_in_temp_dir();
    let first = app.transfers.start_batch();
    let second = app.transfers.start_batch();
    app.planning.push(planning_scan(first, "one"));
    app.planning.push(planning_scan(second, "two"));

    app.apply_transfer_event(TransferEvent::PlanCancelled {
        batch_id: first,
        session_id: 1,
        direction: Direction::Upload,
    });

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
    let batch_id = app.transfers.start_batch();
    let active = app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        10,
        Some(batch_id),
    );
    let queued = app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/b.txt"),
        "/remote/b.txt".to_string(),
        "b.txt".to_string(),
        10,
        Some(batch_id),
    );
    app.transfers.get_mut(active).unwrap().status = JobStatus::InProgress;
    let transfer_cancel = Arc::new(AtomicBool::new(false));
    app.active_transfer_cancel = Some(transfer_cancel.clone());

    app.cancel_all_copies();

    assert!(app.planning[0].cancel.load(Ordering::Relaxed));
    assert!(transfer_cancel.load(Ordering::Relaxed));
    assert_eq!(app.transfers.get(queued).unwrap().status, JobStatus::Cancelled);
}

#[test]
fn cancel_active_transfer_leaves_running_scans_alone() {
    let (_dir, mut app) = app_in_temp_dir();
    app.planning.push(planning_scan(0, "other session's folder"));

    app.cancel_active_transfer();

    assert!(!app.planning[0].cancel.load(Ordering::Relaxed));
}

#[test]
fn cancel_all_copies_with_nothing_running_is_a_no_op() {
    let (_dir, mut app) = app_in_temp_dir();

    app.cancel_all_copies();

    assert!(app.planning.is_empty());
    assert!(app.notifications.current().is_none());
}
