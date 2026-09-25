use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn sample_form() -> FormDialog {
    FormDialog::new(
        "Add connection",
        vec![
            FormField::text("name", "Name", ""),
            FormField::text("host", "Host", ""),
            FormField::masked("password", "Password", ""),
        ],
    )
}

fn choice_form() -> FormDialog {
    FormDialog::new(
        "Add connection",
        vec![
            FormField::choice(
                "protocol",
                "Protocol",
                vec![("sftp".into(), "SFTP".into()), ("ftp".into(), "FTP".into()), ("s3".into(), "S3".into())],
                "ftp",
            ),
            FormField::text("name", "Name", ""),
        ],
    )
}

fn rendered(form: &FormDialog) -> String {
    let backend = ratatui::backend::TestBackend::new(60, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_form(frame, frame.area(), form)).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
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
fn enter_submits_every_fields_key_and_value_in_order() {
    let mut form = sample_form();
    form.fields[0].value = "prod".to_string();
    form.fields[1].value = "server.example.com".to_string();
    form.fields[2].value = "secret".to_string();

    assert_eq!(
        form.handle_key(key(KeyCode::Enter)),
        FormOutcome::Submitted(vec![
            ("name", "prod".to_string()),
            ("host", "server.example.com".to_string()),
            ("password", "secret".to_string()),
        ])
    );
}

#[test]
fn a_choice_starts_on_the_given_value_and_falls_back_to_the_first() {
    assert_eq!(choice_form().value("protocol").as_deref(), Some("ftp"));
    let unknown = FormField::choice("p", "P", vec![("a".into(), "A".into()), ("b".into(), "B".into())], "zzz");
    assert_eq!(unknown.submitted_value(), "a");
}

#[test]
fn left_and_right_cycle_a_focused_choice_with_wrapping_and_report_the_change() {
    let mut form = choice_form();

    assert_eq!(form.handle_key(key(KeyCode::Right)), FormOutcome::ChoiceChanged { key: "protocol" });
    assert_eq!(form.value("protocol").as_deref(), Some("s3"));
    form.handle_key(key(KeyCode::Right));
    assert_eq!(form.value("protocol").as_deref(), Some("sftp"));
    form.handle_key(key(KeyCode::Left));
    assert_eq!(form.value("protocol").as_deref(), Some("s3"));
}

#[test]
fn typing_and_deleting_do_nothing_on_a_choice() {
    let mut form = choice_form();

    for code in [KeyCode::Char('x'), KeyCode::Backspace, KeyCode::Delete, KeyCode::Home, KeyCode::End] {
        assert_eq!(form.handle_key(key(code)), FormOutcome::Pending);
    }
    assert_eq!(form.value("protocol").as_deref(), Some("ftp"));
}

#[test]
fn a_choice_submits_its_value_not_its_label() {
    let mut form = choice_form();

    assert_eq!(
        form.handle_key(key(KeyCode::Enter)),
        FormOutcome::Submitted(vec![("protocol", "ftp".to_string()), ("name", String::new())])
    );
}

#[test]
fn a_choice_renders_its_label_between_arrows() {
    let content = rendered(&choice_form());

    assert!(content.contains("Protocol: ◀ FTP ▶"), "{content}");
}

#[test]
fn the_form_no_longer_shows_a_fixed_protocol_line() {
    assert!(!rendered(&sample_form()).contains("Protocol: SFTP"));
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
    terminal.draw(|frame| render_form(frame, frame.area(), &form)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("******"));
    assert!(!content.contains("secret"));
}

#[test]
fn error_message_renders_when_set() {
    let mut form = sample_form();
    form.error = Some("Host can't be empty".to_string());

    let backend = ratatui::backend::TestBackend::new(60, 10);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_form(frame, frame.area(), &form)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("Host can't be empty"));
}
