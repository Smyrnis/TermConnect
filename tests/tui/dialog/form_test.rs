use super::*;
use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn sample_form() -> FormDialog {
    FormDialog::new(
        "Add connection",
        vec![
            FormField::new("Name", ""),
            FormField::new("Host", ""),
            FormField::new_masked("Password", ""),
        ],
    )
}

#[test]
fn new_form_starts_focused_on_the_first_field() {
    let form = sample_form();
    assert_eq!(form.focused, 0);
}

#[test]
fn typing_inserts_into_the_focused_field_only() {
    let mut form = sample_form();
    form.handle_key(key(KeyCode::Char('a')));
    assert_eq!(form.fields[0].value, "a");
    assert_eq!(form.fields[1].value, "");
}

#[test]
fn tab_moves_focus_to_the_next_field_and_wraps() {
    let mut form = sample_form();
    form.handle_key(key(KeyCode::Tab));
    assert_eq!(form.focused, 1);
    form.handle_key(key(KeyCode::Tab));
    assert_eq!(form.focused, 2);
    form.handle_key(key(KeyCode::Tab));
    assert_eq!(form.focused, 0);
}

#[test]
fn backtab_moves_focus_to_the_previous_field_and_wraps() {
    let mut form = sample_form();
    form.handle_key(key(KeyCode::BackTab));
    assert_eq!(form.focused, 2);
}

#[test]
fn tab_then_typing_inserts_into_the_newly_focused_field() {
    let mut form = sample_form();
    form.handle_key(key(KeyCode::Tab));
    form.handle_key(key(KeyCode::Char('h')));
    assert_eq!(form.fields[1].value, "h");
}

#[test]
fn enter_submits_every_fields_value_in_order() {
    let mut form = sample_form();
    form.fields[0].value = "prod".to_string();
    form.fields[1].value = "server.example.com".to_string();
    form.fields[2].value = "secret".to_string();

    let outcome = form.handle_key(key(KeyCode::Enter));

    assert_eq!(
        outcome,
        FormOutcome::Submitted(vec![
            "prod".to_string(),
            "server.example.com".to_string(),
            "secret".to_string(),
        ])
    );
}

#[test]
fn esc_cancels() {
    let mut form = sample_form();
    assert_eq!(form.handle_key(key(KeyCode::Esc)), FormOutcome::Cancelled);
}

#[test]
fn backspace_removes_from_the_focused_field_at_the_cursor() {
    let mut form = sample_form();
    form.fields[0].value = "ab".to_string();
    form.fields[0].cursor = 2;
    form.handle_key(key(KeyCode::Backspace));
    assert_eq!(form.fields[0].value, "a");
}

#[test]
fn masked_field_renders_asterisks_not_the_value() {
    let mut form = sample_form();
    form.handle_key(key(KeyCode::Tab));
    form.handle_key(key(KeyCode::Tab));
    for c in "secret".chars() {
        form.handle_key(key(KeyCode::Char(c)));
    }

    let backend = ratatui::backend::TestBackend::new(60, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_form(frame, frame.area(), &form))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(content.contains("******"));
    assert!(!content.contains("secret"));
}

#[test]
fn error_message_renders_when_set() {
    let mut form = sample_form();
    form.error = Some("Host can't be empty".to_string());

    let backend = ratatui::backend::TestBackend::new(60, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_form(frame, frame.area(), &form))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(content.contains("Host can't be empty"));
}
