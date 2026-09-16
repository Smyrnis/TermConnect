use super::*;
use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
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
    assert_eq!(
        dialog.handle_key(key(KeyCode::Enter)),
        ConfirmOutcome::Cancelled
    );

    dialog.handle_key(key(KeyCode::Tab));
    assert_eq!(
        dialog.handle_key(key(KeyCode::Enter)),
        ConfirmOutcome::Confirmed
    );
}

#[test]
fn y_and_n_shortcuts_work_regardless_of_focus() {
    let mut dialog = ConfirmDialog::new("Delete?");
    assert_eq!(
        dialog.handle_key(key(KeyCode::Char('y'))),
        ConfirmOutcome::Confirmed
    );

    let mut dialog = ConfirmDialog::new("Delete?");
    assert_eq!(
        dialog.handle_key(key(KeyCode::Char('n'))),
        ConfirmOutcome::Cancelled
    );
}
