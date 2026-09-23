use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn dialog(is_dir: bool, index: usize, total: usize) -> ConflictDialog {
    ConflictDialog { file_name: "report.pdf".to_string(), existing: ExistingFile { size: 2_200_000, modified: Some(1_000_000 - 3 * 86_400), is_dir }, new_size: 2_500_000, new_modified: Some(1_000_000 - 120), index, total, apply_to_rest: false, now: 1_000_000 }
}

fn resolved(resolution: Option<Resolution>, apply_to_rest: bool) -> ConflictOutcome {
    ConflictOutcome::Resolved { resolution, apply_to_rest }
}

#[test]
fn letters_choose_a_resolution() {
    assert_eq!(dialog(false, 0, 1).handle_key(key(KeyCode::Char('o'))), resolved(Some(Resolution::Overwrite), false));
    assert_eq!(dialog(false, 0, 1).handle_key(key(KeyCode::Char('s'))), resolved(Some(Resolution::Skip), false));
    assert_eq!(dialog(false, 0, 1).handle_key(key(KeyCode::Char('r'))), resolved(Some(Resolution::Rename), false));
    assert_eq!(dialog(false, 0, 1).handle_key(key(KeyCode::Char('c'))), resolved(None, false));
    assert_eq!(dialog(false, 0, 1).handle_key(key(KeyCode::Esc)), resolved(None, false));
}

#[test]
fn enter_does_nothing() {
    assert_eq!(dialog(false, 0, 1).handle_key(key(KeyCode::Enter)), ConflictOutcome::Pending);
}

#[test]
fn o_is_ignored_when_the_existing_item_is_a_folder() {
    assert_eq!(dialog(true, 0, 1).handle_key(key(KeyCode::Char('o'))), ConflictOutcome::Pending);
}

#[test]
fn a_toggles_the_same_answer_for_the_rest() {
    let mut conflict = dialog(false, 0, 3);

    assert_eq!(conflict.handle_key(key(KeyCode::Char('a'))), ConflictOutcome::Pending);
    assert_eq!(conflict.handle_key(key(KeyCode::Char('s'))), resolved(Some(Resolution::Skip), true));
}

#[test]
fn a_does_nothing_on_the_last_conflict() {
    let mut conflict = dialog(false, 2, 3);

    conflict.handle_key(key(KeyCode::Char('a')));

    assert!(!conflict.apply_to_rest);
}

#[test]
fn format_age_buckets() {
    assert_eq!(format_age(-5), "just now");
    assert_eq!(format_age(59), "just now");
    assert_eq!(format_age(60), "1 minute ago");
    assert_eq!(format_age(7_200), "2 hours ago");
    assert_eq!(format_age(3 * 86_400), "3 days ago");
}

fn render(conflict: &ConflictDialog) -> String {
    let backend = TestBackend::new(80, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_conflict(frame, frame.area(), conflict)).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn renders_both_files_the_counter_and_the_options() {
    let content = render(&dialog(false, 0, 3));

    assert!(content.contains("\"report.pdf\" already exists (1 of 3)"));
    assert!(content.contains("modified 3 days ago"));
    assert!(content.contains("modified 2 minutes ago"));
    assert!(content.contains("[O]verwrite"));
    assert!(content.contains("same answer for the other 2"));
}

#[test]
fn a_folder_conflict_hides_overwrite() {
    let content = render(&dialog(true, 0, 1));

    assert!(content.contains("folder"));
    assert!(!content.contains("[O]verwrite"));
    assert!(!content.contains("same answer"));
}

#[test]
fn ctrl_shortcuts_do_not_answer_except_ctrl_c() {
    let ctrl = |code| KeyEvent::new(code, KeyModifiers::CONTROL);

    assert_eq!(dialog(false, 0, 3).handle_key(ctrl(KeyCode::Char('r'))), ConflictOutcome::Pending);
    assert_eq!(dialog(false, 0, 3).handle_key(ctrl(KeyCode::Char('s'))), ConflictOutcome::Pending);
    assert_eq!(dialog(false, 0, 3).handle_key(ctrl(KeyCode::Char('o'))), ConflictOutcome::Pending);
    assert_eq!(dialog(false, 0, 3).handle_key(ctrl(KeyCode::Char('a'))), ConflictOutcome::Pending);
    assert_eq!(dialog(false, 0, 3).handle_key(ctrl(KeyCode::Char('c'))), resolved(None, false));
}
