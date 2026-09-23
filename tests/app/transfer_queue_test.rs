use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::*;

fn app_on_transfers_screen() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::at(dir.path().to_path_buf()).unwrap();
    app.apply_action(Action::OpenTransfers);
    (dir, app)
}

fn add_job(app: &mut App, name: &str, batch_id: Option<u64>, status: JobStatus) -> u64 {
    let id = app.transfers.enqueue(
        999,
        Direction::Upload,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        10,
        batch_id,
    );
    app.transfers.get_mut(id).unwrap().status = status;
    id
}

fn flag(app: &mut App, id: u64) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    app.transfer_cancels.insert(id, cancel.clone());
    cancel
}

fn scan(batch_id: u64, name: &str) -> PlanningScan {
    PlanningScan {
        batch_id,
        session_id: 1,
        direction: Direction::Upload,
        display_name: name.to_string(),
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

#[test]
fn ctrl_t_opens_the_transfers_screen_from_files_and_connections() {
    let (_dir, mut app) = app_on_transfers_screen();
    assert_eq!(app.screen, Screen::Transfers);

    app.screen = Screen::Connections;
    app.apply_action(Action::OpenTransfers);

    assert_eq!(app.screen, Screen::Transfers);
}

#[test]
fn esc_returns_to_files() {
    let (_dir, mut app) = app_on_transfers_screen();

    app.apply_action(Action::Back);

    assert_eq!(app.screen, Screen::Files);
}

#[test]
fn cancel_on_a_batch_row_cancels_only_that_batch() {
    let (_dir, mut app) = app_on_transfers_screen();
    let batch = app.transfers.start_batch("photos".to_string());
    let queued = add_job(&mut app, "a", Some(batch), JobStatus::Queued);
    let running = add_job(&mut app, "b", Some(batch), JobStatus::InProgress);
    let running_flag = flag(&mut app, running);
    let completed = add_job(&mut app, "c", Some(batch), JobStatus::Completed);
    let other_batch = app.transfers.start_batch("docs".to_string());
    let other = add_job(&mut app, "d", Some(other_batch), JobStatus::Queued);

    app.apply_action(Action::Delete);

    assert_eq!(app.transfers.get(queued).unwrap().status, JobStatus::Cancelled);
    assert!(running_flag.load(Ordering::Relaxed));
    assert_eq!(app.transfers.get(running).unwrap().status, JobStatus::InProgress);
    assert_eq!(app.transfers.get(completed).unwrap().status, JobStatus::Completed);
    assert_eq!(app.transfers.get(other).unwrap().status, JobStatus::Queued);
}

#[test]
fn cancel_on_a_scan_row_flags_only_that_scan() {
    let (_dir, mut app) = app_on_transfers_screen();
    app.planning.push(scan(1, "one"));
    app.planning.push(scan(2, "two"));
    app.transfers_cursor = 1;

    app.apply_action(Action::Delete);

    assert!(!app.planning[0].cancel.load(Ordering::Relaxed));
    assert!(app.planning[1].cancel.load(Ordering::Relaxed));
}

#[test]
fn cancel_on_a_finished_row_does_nothing() {
    let (_dir, mut app) = app_on_transfers_screen();
    let done = add_job(&mut app, "a", None, JobStatus::Completed);

    app.apply_action(Action::Delete);

    assert_eq!(app.transfers.get(done).unwrap().status, JobStatus::Completed);
    assert!(app.notifications.current().is_none());
}

#[test]
fn retry_on_a_disconnected_session_shows_one_warning_and_leaves_the_files() {
    let (_dir, mut app) = app_on_transfers_screen();
    let batch = app.transfers.start_batch("photos".to_string());
    let failed = add_job(&mut app, "a", Some(batch), JobStatus::Failed("boom".to_string()));
    app.transfers.get_mut(failed).unwrap().attempts = 3;
    let cancelled = add_job(&mut app, "b", Some(batch), JobStatus::Cancelled);

    app.apply_action(Action::Open);

    let job = app.transfers.get(failed).unwrap();
    assert_eq!((job.status.clone(), job.attempts), (JobStatus::Failed("boom".to_string()), 3));
    assert_eq!(app.transfers.get(cancelled).unwrap().status, JobStatus::Cancelled);
    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Warning);
    assert_eq!(notification.message, "Can't retry: session disconnected");
    app.notifications.dismiss_current();
    assert!(app.notifications.current().is_none());
}

#[test]
fn cancelling_the_last_pending_row_refreshes_its_destination() {
    let (dir, mut app) = app_on_transfers_screen();
    let done = app.transfers.enqueue(
        1,
        Direction::Download,
        dir.path().join("done.txt"),
        "/remote/done.txt".to_string(),
        "done.txt".to_string(),
        10,
        None,
    );
    app.transfers.get_mut(done).unwrap().status = JobStatus::Completed;
    std::fs::write(dir.path().join("done.txt"), b"x").unwrap();
    app.transfers.enqueue(
        1,
        Direction::Download,
        dir.path().join("later.txt"),
        "/remote/later.txt".to_string(),
        "later.txt".to_string(),
        10,
        None,
    );
    app.transfers_cursor = 1;

    app.apply_action(Action::Delete);

    assert!(
        app.local
            .rows()
            .iter()
            .any(|row| matches!(row, crate::tui::panels::Row::Entry(entry) if entry.name == "done.txt"))
    );
}

#[test]
fn retry_leaves_running_jobs_alone() {
    let (_dir, mut app) = app_on_transfers_screen();
    let batch = app.transfers.start_batch("photos".to_string());
    let running = add_job(&mut app, "a", Some(batch), JobStatus::InProgress);
    app.transfers.get_mut(running).unwrap().attempts = 1;
    add_job(&mut app, "b", Some(batch), JobStatus::Failed("boom".to_string()));

    app.apply_action(Action::Open);

    let job = app.transfers.get(running).unwrap();
    assert_eq!((job.status.clone(), job.attempts), (JobStatus::InProgress, 1));
}

#[test]
fn clear_removes_only_finished_rows_and_clamps_the_cursor() {
    let (_dir, mut app) = app_on_transfers_screen();
    let done = add_job(&mut app, "a", None, JobStatus::Completed);
    let running = add_job(&mut app, "b", None, JobStatus::InProgress);
    let batch = app.transfers.start_batch("photos".to_string());
    let failed_in_batch = add_job(&mut app, "c", Some(batch), JobStatus::Failed("boom".to_string()));
    app.transfers_cursor = 2;

    app.apply_action(Action::Refresh);

    assert!(app.transfers.get(done).is_none());
    assert!(app.transfers.get(failed_in_batch).is_none());
    assert!(app.transfers.get(running).is_some());
    assert_eq!(app.transfer_rows().len(), 1);
    assert_eq!(app.transfers_cursor, 0);
}

#[test]
fn clear_keeps_a_scanning_copy() {
    let (_dir, mut app) = app_on_transfers_screen();
    let batch = app.transfers.start_batch("photos".to_string());
    app.planning.push(scan(batch, "photos"));
    add_job(&mut app, "a", None, JobStatus::Completed);

    app.apply_action(Action::Refresh);

    assert_eq!(app.transfer_rows().len(), 1);
    assert_eq!(app.transfers.batch_label(batch), Some("photos"));
}

#[test]
fn actions_on_an_empty_list_do_nothing() {
    let (_dir, mut app) = app_on_transfers_screen();

    for action in [Action::Up, Action::Down, Action::Delete, Action::Open, Action::Refresh] {
        app.apply_action(action);
    }

    assert_eq!(app.transfers_cursor, 0);
    assert!(app.notifications.current().is_none());
    assert_eq!(app.screen, Screen::Transfers);
}

#[test]
fn up_and_down_stay_within_the_rows() {
    let (_dir, mut app) = app_on_transfers_screen();
    add_job(&mut app, "a", None, JobStatus::Queued);
    add_job(&mut app, "b", None, JobStatus::Queued);

    for _ in 0..3 {
        app.apply_action(Action::Down);
    }
    assert_eq!(app.transfers_cursor, 1);
    for _ in 0..3 {
        app.apply_action(Action::Up);
    }
    assert_eq!(app.transfers_cursor, 0);
}

#[test]
fn cursor_past_the_end_acts_on_the_last_row() {
    let (_dir, mut app) = app_on_transfers_screen();
    let only = add_job(&mut app, "a", None, JobStatus::Queued);
    app.transfers_cursor = 5;

    app.apply_action(Action::Delete);

    assert_eq!(app.transfers.get(only).unwrap().status, JobStatus::Cancelled);
}

#[test]
fn the_transfers_screen_renders_its_rows() {
    use ratatui::{Terminal, backend::TestBackend};

    let (_dir, mut app) = app_on_transfers_screen();
    add_job(&mut app, "report.pdf", None, JobStatus::Queued);
    let backend = TestBackend::new(100, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| app.render(frame)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(content.contains("Transfers (1)"));
    assert!(content.contains("report.pdf"));
}

#[test]
fn clearing_keeps_the_cursor_on_the_same_row() {
    let (_dir, mut app) = app_on_transfers_screen();
    add_job(&mut app, "a", None, JobStatus::Completed);
    let selected = add_job(&mut app, "b", None, JobStatus::InProgress);
    add_job(&mut app, "c", None, JobStatus::InProgress);
    add_job(&mut app, "d", None, JobStatus::Completed);
    app.transfers_cursor = 1;

    app.apply_action(Action::Refresh);

    assert_eq!(app.transfer_rows()[app.transfers_cursor].job_ids, vec![selected]);
}

#[test]
fn clearing_removes_partials_only_of_cancelled_and_failed_downloads() {
    let (dir, mut app) = app_on_transfers_screen();
    let mut download = |name: &str, status: JobStatus| {
        let id = app.transfers.enqueue(
            999,
            Direction::Download,
            dir.path().join(name),
            format!("/remote/{name}"),
            name.to_string(),
            10,
            None,
        );
        app.transfers.get_mut(id).unwrap().status = status;
        std::fs::write(dir.path().join(format!("{name}.part")), b"x").unwrap();
    };
    download("cancelled.bin", JobStatus::Cancelled);
    download("failed.bin", JobStatus::Failed("boom".to_string()));
    download("done.bin", JobStatus::Completed);
    download("queued.bin", JobStatus::Queued);

    app.apply_action(Action::Refresh);

    assert!(!dir.path().join("cancelled.bin.part").exists());
    assert!(!dir.path().join("failed.bin.part").exists());
    assert!(dir.path().join("done.bin.part").exists());
    assert!(dir.path().join("queued.bin.part").exists());
}

#[test]
fn clearing_keeps_a_partial_that_a_queued_copy_of_the_same_file_uses() {
    let (dir, mut app) = app_on_transfers_screen();
    for status in [JobStatus::Cancelled, JobStatus::Queued] {
        let id = app.transfers.enqueue(
            999,
            Direction::Download,
            dir.path().join("big.iso"),
            "/remote/big.iso".to_string(),
            "big.iso".to_string(),
            10,
            None,
        );
        app.transfers.get_mut(id).unwrap().status = status;
    }
    std::fs::write(dir.path().join("big.iso.part"), b"x").unwrap();

    app.apply_action(Action::Refresh);

    assert!(dir.path().join("big.iso.part").exists());
}

#[test]
fn clearing_never_deletes_a_completed_file_named_like_a_partial() {
    let (dir, mut app) = app_on_transfers_screen();
    let cancelled = app.transfers.enqueue(
        999,
        Direction::Download,
        dir.path().join("foo"),
        "/remote/foo".to_string(),
        "foo".to_string(),
        10,
        None,
    );
    app.transfers.get_mut(cancelled).unwrap().status = JobStatus::Cancelled;
    let completed = app.transfers.enqueue(
        999,
        Direction::Download,
        dir.path().join("foo.part"),
        "/remote/foo.part".to_string(),
        "foo.part".to_string(),
        10,
        None,
    );
    app.transfers.get_mut(completed).unwrap().status = JobStatus::Completed;
    std::fs::write(dir.path().join("foo.part"), b"a real file").unwrap();

    app.apply_action(Action::Refresh);

    assert!(dir.path().join("foo.part").exists());
}
