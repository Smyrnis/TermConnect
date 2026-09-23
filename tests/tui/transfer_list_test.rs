use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::transfer::{
    Direction,
    rows::{QueueRow, RowKind, RowState},
};

fn row(kind: RowKind, label: &str, direction: Direction, files: (usize, usize), bytes: (u64, u64), state: RowState) -> QueueRow {
    QueueRow { kind, label: label.to_string(), direction, files_done: files.0, files_total: files.1, bytes_done: bytes.0, bytes_total: bytes.1, state, job_ids: Vec::new() }
}

fn render(rows: &[QueueRow], cursor: usize, copy_key: Option<&str>) -> String {
    render_at_width(rows, cursor, copy_key, 90)
}

fn render_at_width(rows: &[QueueRow], cursor: usize, copy_key: Option<&str>, width: u16) -> String {
    let backend = TestBackend::new(width, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_transfer_list(frame, frame.area(), rows, cursor, copy_key)).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn renders_batch_single_and_scan_rows() {
    let rows = vec![row(RowKind::Batch(0), "photos", Direction::Upload, (412, 5000), (38, 100), RowState::Running), row(RowKind::Single(9), "report.pdf", Direction::Download, (0, 1), (71, 100), RowState::Queued), row(RowKind::Scan(3), "music", Direction::Upload, (0, 0), (0, 0), RowState::Scanning), row(RowKind::Batch(1), "docs", Direction::Upload, (8, 10), (80, 100), RowState::PartlyFailed(2))];

    let content = render(&rows, 0, Some("F5"));

    assert!(content.contains("Transfers (4)"));
    assert!(content.contains("\u{2191} photos"));
    assert!(content.contains("412/5000 files"));
    assert!(content.contains("38%"));
    assert!(content.contains("running"));
    assert!(content.contains("\u{2193} report.pdf"));
    assert!(content.contains("71%"));
    assert!(content.contains("scanning\u{2026}"));
    assert!(content.contains("done, 2 failed"));
}

#[test]
fn shows_the_empty_message_with_the_live_copy_key() {
    let content = render(&[], 0, Some("F6"));

    assert!(content.contains("Transfers (0)"));
    assert!(content.contains("No transfers \u{2014} F6 copies the selected file or folder"));
}

#[test]
fn renders_with_a_cursor_past_the_end() {
    let rows = vec![row(RowKind::Single(0), "a.txt", Direction::Upload, (0, 1), (0, 10), RowState::Queued)];

    let content = render(&rows, 5, Some("F5"));

    assert!(content.contains("a.txt"));
}

#[test]
fn long_state_and_amount_texts_stay_fully_visible() {
    let rows = vec![row(RowKind::Batch(0), "docs", Direction::Upload, (12345, 100000), (50, 100), RowState::PartlyFailed(12345))];

    let content = render(&rows, 0, Some("F5"));

    assert!(content.contains("12345/100000 files"));
    assert!(content.contains("done, 12345 failed"));
}

#[test]
fn a_narrow_terminal_keeps_the_state_column() {
    let rows = vec![row(RowKind::Batch(0), "photos", Direction::Upload, (412, 5000), (38, 100), RowState::Running)];

    let content = render_at_width(&rows, 0, Some("F5"), 40);

    assert!(content.contains("running"));
    assert!(content.contains("\u{2191} ph"));
}

#[test]
fn shows_a_plain_empty_message_when_copy_is_unbound() {
    let content = render(&[], 0, None);

    assert!(content.contains("No transfers yet"));
    assert!(!content.contains("copies"));
}

#[test]
fn a_copy_awaiting_answers_says_waiting_for_you() {
    let rows = vec![row(RowKind::Scan(1), "photos", Direction::Upload, (0, 0), (0, 0), RowState::AwaitingAnswer)];

    assert!(render(&rows, 0, Some("F5")).contains("waiting for you"));
}
