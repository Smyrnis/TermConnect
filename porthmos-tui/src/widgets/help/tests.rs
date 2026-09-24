use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::input::KeyBindings;

#[test]
fn renders_every_action_with_its_bound_key() {
    let bindings = KeyBindings::defaults();
    let backend = TestBackend::new(50, 25);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_help(frame, frame.area(), &bindings)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("quit"));
    assert!(content.contains("F10"));
}

#[test]
fn lists_open_transfers() {
    let bindings = KeyBindings::defaults();
    let backend = TestBackend::new(50, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_help(frame, frame.area(), &bindings)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("open_transfers"));
}
