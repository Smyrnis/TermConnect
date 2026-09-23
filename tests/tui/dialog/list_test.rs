use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn dialog() -> ListDialog {
    ListDialog::new("Bookmarks", vec!["a".to_string(), "b".to_string(), "c".to_string()])
}

#[test]
fn down_moves_the_cursor_forward_and_clamps_at_the_end() {
    let mut dialog = dialog();
    dialog.handle_key(key(KeyCode::Down));
    dialog.handle_key(key(KeyCode::Down));
    dialog.handle_key(key(KeyCode::Down));
    assert_eq!(dialog.cursor, 2);
}

#[test]
fn up_clamps_at_zero() {
    let mut dialog = dialog();
    dialog.handle_key(key(KeyCode::Up));
    assert_eq!(dialog.cursor, 0);
}

#[test]
fn enter_selects_the_entry_under_the_cursor() {
    let mut dialog = dialog();
    dialog.handle_key(key(KeyCode::Down));
    assert_eq!(dialog.handle_key(key(KeyCode::Enter)), ListOutcome::Selected(1));
}

#[test]
fn esc_cancels() {
    let mut dialog = dialog();
    assert_eq!(dialog.handle_key(key(KeyCode::Esc)), ListOutcome::Cancelled);
}

#[test]
fn f8_removes_only_when_the_dialog_is_removable() {
    let mut dialog = dialog();
    assert_eq!(dialog.handle_key(key(KeyCode::F(8))), ListOutcome::Pending);

    let mut removable = dialog.removable(true);
    assert_eq!(removable.handle_key(key(KeyCode::F(8))), ListOutcome::Removed(0));
}

#[test]
fn enter_on_an_empty_list_does_nothing() {
    let mut dialog = ListDialog::new("Bookmarks", Vec::new());
    assert_eq!(dialog.handle_key(key(KeyCode::Enter)), ListOutcome::Pending);
}
