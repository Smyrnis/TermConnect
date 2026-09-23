use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

#[test]
fn typing_characters_appends_to_value() {
    let mut dialog = TextInputDialog::new("New name", "");
    dialog.handle_key(key(KeyCode::Char('a')));
    dialog.handle_key(key(KeyCode::Char('b')));
    assert_eq!(dialog.value, "ab");
}

#[test]
fn backspace_removes_the_last_character() {
    let mut dialog = TextInputDialog::new("New name", "ab");
    dialog.handle_key(key(KeyCode::Backspace));
    assert_eq!(dialog.value, "a");
}

#[test]
fn enter_submits_the_current_value() {
    let mut dialog = TextInputDialog::new("New name", "final");
    let outcome = dialog.handle_key(key(KeyCode::Enter));
    assert_eq!(outcome, TextInputOutcome::Submitted("final".to_string()));
}

#[test]
fn esc_cancels() {
    let mut dialog = TextInputDialog::new("New name", "final");
    let outcome = dialog.handle_key(key(KeyCode::Esc));
    assert_eq!(outcome, TextInputOutcome::Cancelled);
}

#[test]
fn new_masked_starts_empty_and_marks_masked() {
    let dialog = TextInputDialog::new_masked("Password");
    assert_eq!(dialog.value, "");
    assert!(dialog.masked);
}

#[test]
fn masked_dialog_renders_asterisks_not_the_value() {
    let mut dialog = TextInputDialog::new_masked("Password");
    dialog.handle_key(key(KeyCode::Char('s')));
    dialog.handle_key(key(KeyCode::Char('e')));
    dialog.handle_key(key(KeyCode::Char('t')));

    let backend = ratatui::backend::TestBackend::new(60, 6);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_text_input(frame, frame.area(), &dialog)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("***"));
    assert!(!content.contains("set"));
}

#[test]
fn left_and_right_move_the_cursor_without_changing_the_value() {
    let mut dialog = TextInputDialog::new("Name", "abc");
    assert_eq!(dialog.cursor, 3);

    dialog.handle_key(key(KeyCode::Left));
    dialog.handle_key(key(KeyCode::Left));
    assert_eq!(dialog.cursor, 1);

    dialog.handle_key(key(KeyCode::Right));
    assert_eq!(dialog.cursor, 2);
    assert_eq!(dialog.value, "abc");
}

#[test]
fn home_and_end_jump_to_the_boundaries() {
    let mut dialog = TextInputDialog::new("Name", "abc");
    dialog.handle_key(key(KeyCode::Home));
    assert_eq!(dialog.cursor, 0);
    dialog.handle_key(key(KeyCode::End));
    assert_eq!(dialog.cursor, 3);
}

#[test]
fn typing_inserts_at_the_cursor_not_only_at_the_end() {
    let mut dialog = TextInputDialog::new("Name", "ac");
    dialog.cursor = 1;
    dialog.handle_key(key(KeyCode::Char('b')));
    assert_eq!(dialog.value, "abc");
    assert_eq!(dialog.cursor, 2);
}

#[test]
fn delete_removes_the_character_at_the_cursor() {
    let mut dialog = TextInputDialog::new("Name", "abc");
    dialog.cursor = 0;
    dialog.handle_key(key(KeyCode::Delete));
    assert_eq!(dialog.value, "bc");
    assert_eq!(dialog.cursor, 0);
}

#[test]
fn backspace_at_the_start_does_nothing() {
    let mut dialog = TextInputDialog::new("Name", "abc");
    dialog.cursor = 0;
    dialog.handle_key(key(KeyCode::Backspace));
    assert_eq!(dialog.value, "abc");
    assert_eq!(dialog.cursor, 0);
}

#[test]
fn cursor_stays_within_bounds_on_an_empty_value() {
    let mut dialog = TextInputDialog::new("Name", "");
    dialog.handle_key(key(KeyCode::Left));
    assert_eq!(dialog.cursor, 0);
    dialog.handle_key(key(KeyCode::Right));
    assert_eq!(dialog.cursor, 0);
}
