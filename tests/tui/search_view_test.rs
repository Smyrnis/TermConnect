use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use std::path::PathBuf;

use crate::filesystem::Entry;

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn entry(name: &str) -> Entry {
    Entry {
        name: name.to_string(),
        path: PathBuf::from(format!("/{name}")),
        is_dir: false,
        size: 0,
        permissions: None,
    }
}

#[test]
fn typing_inserts_at_the_cursor_and_reports_pattern_changed() {
    let mut view = SearchView::new();
    assert_eq!(
        view.handle_key(key(KeyCode::Char('a'))),
        SearchOutcome::PatternChanged
    );
    assert_eq!(view.pattern, "a");
    assert_eq!(view.cursor, 1);
}

#[test]
fn ctrl_modified_characters_are_not_inserted() {
    let mut view = SearchView::new();
    let event = KeyEvent {
        code: KeyCode::Char('c'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    assert_eq!(view.handle_key(event), SearchOutcome::Pending);
    assert_eq!(view.pattern, "");
}

#[test]
fn down_and_up_move_the_selection_within_bounds() {
    let mut view = SearchView::new();
    view.push_result(entry("a"));
    view.push_result(entry("b"));

    view.handle_key(key(KeyCode::Down));
    assert_eq!(view.selected, 1);
    view.handle_key(key(KeyCode::Down)); // clamps at the last result
    assert_eq!(view.selected, 1);
    view.handle_key(key(KeyCode::Up));
    assert_eq!(view.selected, 0);
}

#[test]
fn start_clears_previous_results_and_state() {
    let mut view = SearchView::new();
    view.push_result(entry("a"));
    view.finish(true);

    view.start();

    assert!(view.results.is_empty());
    assert!(!view.truncated);
    assert_eq!(view.selected, 0);
}

#[test]
fn esc_reports_cancel() {
    let mut view = SearchView::new();
    assert_eq!(view.handle_key(key(KeyCode::Esc)), SearchOutcome::Cancel);
}

#[test]
fn enter_reports_open() {
    let mut view = SearchView::new();
    assert_eq!(view.handle_key(key(KeyCode::Enter)), SearchOutcome::Open);
}
