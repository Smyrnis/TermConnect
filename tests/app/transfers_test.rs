use super::*;

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
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
