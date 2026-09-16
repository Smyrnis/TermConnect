use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::tui::input::KeyBindings;

use super::*;

#[test]
fn renders_every_action_with_its_bound_key() {
    let bindings = KeyBindings::defaults();
    let backend = TestBackend::new(50, 25);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_help(frame, frame.area(), &bindings))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(content.contains("quit"));
    assert!(content.contains("F10"));
}
