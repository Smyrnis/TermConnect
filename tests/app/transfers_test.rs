use super::*;
use crate::connection::ConnectionSource;
use crate::transfer::plan::{DirectoryPlan, PlannedFile};

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

fn app_with_a_local_directory_selected() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("myfolder")).unwrap();
    let mut app = App::at(dir.path().to_path_buf()).unwrap();
    // The listing is just [Parent, myfolder] — move the cursor onto
    // myfolder itself, since target_entries() otherwise falls back to
    // whatever row the cursor sits on and the default cursor (0) is the
    // synthetic ".." row.
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

#[test]
fn maybe_start_next_transfer_notifies_when_the_jobs_session_has_disconnected() {
    let (_dir, mut app) = app_in_temp_dir();
    // No session/session_resources entry for `999` exists — simulates a
    // job whose session disconnected before its turn came up.
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
    app.sessions.insert(
        sample_connection_entry(),
        PanelState::from_listing(PathBuf::from("/remote"), Vec::new()),
    );
    // sessions.active() now succeeds, but there's no matching entry in
    // app.session_resources — simulates a session whose SFTP handle is
    // gone. Before this task, this scenario hit the old "isn't supported
    // yet" directory-skip path instead (session_resources was never even
    // consulted for a skipped directory), so this is a meaningful RED:
    // today the message wouldn't mention "disconnected" at all.

    app.start_copy();

    assert!(app.planning.is_none());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Error);
    assert!(notification.message.contains("disconnected"));
    assert!(app.transfers.next_to_run().is_none());
}

#[test]
fn plan_ready_enqueues_every_planned_file_under_the_batch_id() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    app.planning = Some((batch_id, "myfolder".to_string()));
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

    app.apply_transfer_event(TransferEvent::PlanReady {
        batch_id,
        session_id: 1,
        direction: Direction::Upload,
        plan,
    });

    assert!(app.planning.is_none());
    let progress = app.transfers.batch_progress(batch_id);
    assert_eq!(progress.total_files, 2);
    assert_eq!(progress.total_bytes, 30);
}

#[test]
fn plan_ready_warns_once_about_skipped_symlinks() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    app.planning = Some((batch_id, "myfolder".to_string()));
    let plan = DirectoryPlan {
        files: Vec::new(),
        skipped_symlinks: 3,
    };

    app.apply_transfer_event(TransferEvent::PlanReady {
        batch_id,
        session_id: 1,
        direction: Direction::Upload,
        plan,
    });

    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Warning);
    assert!(notification.message.contains("3 symlinks"));
}

#[test]
fn plan_failed_clears_planning_and_shows_an_error() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    app.planning = Some((batch_id, "myfolder".to_string()));

    app.apply_transfer_event(TransferEvent::PlanFailed {
        batch_id,
        message: "Copy failed: permission denied".to_string(),
    });

    assert!(app.planning.is_none());
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Error);
    assert_eq!(notification.message, "Copy failed: permission denied");
}
