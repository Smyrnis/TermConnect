use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

#[test]
fn enter_and_esc_close_and_other_keys_wait() {
    let mut dialog = MessageDialog::new("T", "M");
    assert_eq!(dialog.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)), MessageOutcome::Pending);
    assert_eq!(dialog.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), MessageOutcome::Closed);
    assert_eq!(dialog.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)), MessageOutcome::Closed);
}

#[test]
fn it_renders_the_title_and_wrapped_message() {
    let dialog = MessageDialog::new("Missing hosts", "These labelled hosts are no longer in ~/.ssh/config: web1.");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(50, 12)).unwrap();
    terminal.draw(|frame| render_message(frame, frame.area(), &dialog)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(screen.contains("Missing hosts"), "{screen}");
    assert!(screen.contains("web1"), "{screen}");
}

#[test]
fn a_message_that_wraps_at_words_still_shows_the_ok_line() {
    let words = format!("{} ", "x".repeat(25)).repeat(6);
    let dialog = MessageDialog::new("T", words.trim());
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(52, 30)).unwrap();
    terminal.draw(|frame| render_message(frame, frame.area(), &dialog)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(screen.contains("[Enter] OK"), "{screen}");
}

#[test]
fn each_line_of_a_multi_line_message_gets_its_own_row() {
    let dialog = MessageDialog::new("Details", "first line\nsecond line\nthird line");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 14)).unwrap();

    terminal.draw(|frame| render_message(frame, frame.area(), &dialog)).unwrap();

    let buffer = terminal.backend().buffer();
    let rows: Vec<String> =
        (0..14).map(|y| (0..60).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>()).collect();
    let row_of = |needle: &str| rows.iter().position(|row| row.contains(needle)).unwrap();
    assert_eq!(row_of("second line"), row_of("first line") + 1);
    assert_eq!(row_of("third line"), row_of("first line") + 2);
    assert!(rows.iter().any(|row| row.contains("[Enter] OK")));
}
