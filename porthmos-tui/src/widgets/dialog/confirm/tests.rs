use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

#[test]
fn default_focus_is_no() {
    let dialog = ConfirmDialog::new("Delete?");
    assert_eq!(dialog.focus, ConfirmFocus::No);
}

#[test]
fn tab_toggles_focus_between_yes_and_no() {
    let mut dialog = ConfirmDialog::new("Delete?");
    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focus, ConfirmFocus::Yes);
    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.focus, ConfirmFocus::No);
}

#[test]
fn enter_confirms_only_when_yes_is_focused() {
    let mut dialog = ConfirmDialog::new("Delete?");
    assert_eq!(dialog.handle_key(key(KeyCode::Enter)), ConfirmOutcome::Cancelled);

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(dialog.handle_key(key(KeyCode::Enter)), ConfirmOutcome::Confirmed);
}

#[test]
fn y_and_n_shortcuts_work_regardless_of_focus() {
    let mut dialog = ConfirmDialog::new("Delete?");
    assert_eq!(dialog.handle_key(key(KeyCode::Char('y'))), ConfirmOutcome::Confirmed);

    let mut dialog = ConfirmDialog::new("Delete?");
    assert_eq!(dialog.handle_key(key(KeyCode::Char('n'))), ConfirmOutcome::Cancelled);
}

fn rendered_rows(dialog: &ConfirmDialog, width: u16, height: u16) -> Vec<String> {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_confirm(frame, frame.area(), dialog)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height).map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect()).collect()
}

#[test]
fn every_line_of_a_multi_line_message_is_shown_on_its_own_row() {
    let dialog = ConfirmDialog::new("first line\nsecond line\nthird line");

    let rows = rendered_rows(&dialog, 40, 12);

    let row_of =
        |text: &str| rows.iter().position(|row| row.contains(text)).unwrap_or_else(|| panic!("{text} missing"));
    assert_eq!(row_of("second line"), row_of("first line") + 1);
    assert_eq!(row_of("third line"), row_of("second line") + 1);
    assert!(row_of("[y] Yes") > row_of("third line"));
}

#[test]
fn a_line_wider_than_the_popup_wraps_instead_of_being_cut_off() {
    let dialog = ConfirmDialog::new(format!("{}\nTrust it?", "x".repeat(100)));

    let rows = rendered_rows(&dialog, 40, 14);

    let shown: usize = rows.iter().map(|row| row.matches('x').count()).sum();
    assert_eq!(shown, 100);
    assert!(rows.iter().any(|row| row.contains("Trust it?")));
    assert!(rows.iter().any(|row| row.contains("[y] Yes")));
}
