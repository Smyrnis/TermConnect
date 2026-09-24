use std::path::{Path, PathBuf};

use termconnect_core::transfer::{
    JobStatus, TransferQueue,
    rows::{RowState, ScanInfo},
};

use super::*;
use crate::app::testing::{TestApp, test_app};

fn app_on_transfers_screen() -> TestApp {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenTransfers);
    test
}

fn add_job(queue: &mut TransferQueue, name: &str, batch_id: Option<u64>, status: JobStatus) -> u64 {
    let id = queue.enqueue(
        999,
        Direction::Upload,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        10,
        batch_id,
    );
    queue.get_mut(id).unwrap().status = status;
    id
}

fn show(test: &mut TestApp, queue: &TransferQueue, scans: &[ScanInfo]) {
    test.app.apply_core_event(Event::TransfersChanged(TransferSnapshot::of(queue, scans)));
}

fn scan(batch_id: u64, label: &str) -> ScanInfo<'_> {
    ScanInfo { batch_id, label, direction: Direction::Upload, state: RowState::Scanning }
}

#[test]
fn ctrl_t_opens_the_transfers_screen_from_files_and_connections() {
    let mut test = app_on_transfers_screen();
    assert_eq!(test.app.screen, Screen::Transfers);

    test.app.screen = Screen::Connections;
    test.app.apply_action(Action::OpenTransfers);

    assert_eq!(test.app.screen, Screen::Transfers);
}

#[test]
fn esc_returns_to_files() {
    let mut test = app_on_transfers_screen();

    test.app.apply_action(Action::Back);

    assert_eq!(test.app.screen, Screen::Files);
}

#[test]
fn cancel_on_a_batch_row_cancels_only_that_batch() {
    let mut test = app_on_transfers_screen();
    let mut queue = TransferQueue::new();
    let batch = queue.start_batch("photos".to_string());
    add_job(&mut queue, "a", Some(batch), JobStatus::Queued);
    let other_batch = queue.start_batch("docs".to_string());
    add_job(&mut queue, "d", Some(other_batch), JobStatus::Queued);
    show(&mut test, &queue, &[]);

    test.app.apply_action(Action::Delete);

    assert_eq!(test.sent(), vec![Command::CancelRow { kind: RowKind::Batch(batch) }]);
}

#[test]
fn cancel_on_a_scan_row_flags_only_that_scan() {
    let mut test = app_on_transfers_screen();
    show(&mut test, &TransferQueue::new(), &[scan(1, "one"), scan(2, "two")]);
    test.app.transfers_cursor = 1;

    test.app.apply_action(Action::Delete);

    assert_eq!(test.sent(), vec![Command::CancelRow { kind: RowKind::Scan(2) }]);
}

#[test]
fn retry_asks_the_core_to_retry_the_selected_row() {
    let mut test = app_on_transfers_screen();
    let mut queue = TransferQueue::new();
    let failed = add_job(&mut queue, "a", None, JobStatus::Failed("boom".to_string()));
    show(&mut test, &queue, &[]);

    test.app.apply_action(Action::Open);

    assert_eq!(test.sent(), vec![Command::RetryRow { kind: RowKind::Single(failed) }]);
}

#[test]
fn clear_removes_only_finished_rows_and_clamps_the_cursor() {
    let mut test = app_on_transfers_screen();
    let mut queue = TransferQueue::new();
    add_job(&mut queue, "a", None, JobStatus::Completed);
    add_job(&mut queue, "b", None, JobStatus::InProgress);
    let batch = queue.start_batch("photos".to_string());
    add_job(&mut queue, "c", Some(batch), JobStatus::Failed("boom".to_string()));
    show(&mut test, &queue, &[]);
    test.app.transfers_cursor = 2;

    test.app.apply_action(Action::Refresh);
    let mut remaining = TransferQueue::new();
    add_job(&mut remaining, "b", None, JobStatus::InProgress);
    show(&mut test, &remaining, &[]);

    assert_eq!(test.sent(), vec![Command::ClearFinished]);
    assert_eq!(test.app.transfer_rows().len(), 1);
    assert_eq!(test.app.transfers_cursor, 0);
}

#[test]
fn clearing_keeps_the_cursor_on_the_same_row() {
    let mut test = app_on_transfers_screen();
    let mut queue = TransferQueue::new();
    add_job(&mut queue, "a", None, JobStatus::Completed);
    let selected = add_job(&mut queue, "b", None, JobStatus::InProgress);
    add_job(&mut queue, "c", None, JobStatus::InProgress);
    add_job(&mut queue, "d", None, JobStatus::Completed);
    show(&mut test, &queue, &[]);
    test.app.transfers_cursor = 1;

    test.app.apply_action(Action::Refresh);
    let finished: Vec<u64> = queue.jobs().filter(|job| job.status == JobStatus::Completed).map(|job| job.id).collect();
    queue.remove_jobs(&finished);
    show(&mut test, &queue, &[]);

    assert_eq!(test.app.transfer_rows()[test.app.transfers_cursor].job_ids, vec![selected]);
}

#[test]
fn actions_on_an_empty_list_do_nothing() {
    let mut test = app_on_transfers_screen();

    for action in [Action::Up, Action::Down, Action::Delete, Action::Open, Action::Refresh] {
        test.app.apply_action(action);
    }

    assert_eq!(test.app.transfers_cursor, 0);
    assert!(test.app.notifications.current().is_none());
    assert_eq!(test.app.screen, Screen::Transfers);
    assert!(test.sent().is_empty());
}

#[test]
fn up_and_down_stay_within_the_rows() {
    let mut test = app_on_transfers_screen();
    let mut queue = TransferQueue::new();
    add_job(&mut queue, "a", None, JobStatus::Queued);
    add_job(&mut queue, "b", None, JobStatus::Queued);
    show(&mut test, &queue, &[]);

    for _ in 0..3 {
        test.app.apply_action(Action::Down);
    }
    assert_eq!(test.app.transfers_cursor, 1);
    for _ in 0..3 {
        test.app.apply_action(Action::Up);
    }
    assert_eq!(test.app.transfers_cursor, 0);
}

#[test]
fn cursor_past_the_end_acts_on_the_last_row() {
    let mut test = app_on_transfers_screen();
    let mut queue = TransferQueue::new();
    let only = add_job(&mut queue, "a", None, JobStatus::Queued);
    show(&mut test, &queue, &[]);
    test.app.transfers_cursor = 5;

    test.app.apply_action(Action::Delete);

    assert_eq!(test.sent(), vec![Command::CancelRow { kind: RowKind::Single(only) }]);
}

#[test]
fn the_transfers_screen_renders_its_rows() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut test = app_on_transfers_screen();
    let mut queue = TransferQueue::new();
    add_job(&mut queue, "report.pdf", None, JobStatus::Queued);
    show(&mut test, &queue, &[]);
    let backend = TestBackend::new(100, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| test.app.render(frame)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(content.contains("Transfers (1)"));
    assert!(content.contains("report.pdf"));
}
