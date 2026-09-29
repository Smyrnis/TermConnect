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

fn press(form: &mut FormDialog, code: KeyCode) -> FormOutcome {
    form.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn screen_of(form: &FormDialog) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 10)).unwrap();
    terminal.draw(|frame| render_form(frame, frame.area(), form)).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn a_saved_secret_is_kept_until_edited() {
    let field = FormField::saved_secret("password", "Password");
    assert_eq!(field.submitted_value(), KEPT_SECRET);

    let mut form = FormDialog::new("t", vec![field]);
    press(&mut form, KeyCode::Char('x'));
    press(&mut form, KeyCode::Char('y'));
    assert_eq!(form.fields[0].submitted_value(), "xy");
}

#[test]
fn backspace_or_delete_on_a_saved_secret_clears_it() {
    for code in [KeyCode::Backspace, KeyCode::Delete] {
        let mut form = FormDialog::new("t", vec![FormField::saved_secret("password", "Password")]);
        press(&mut form, code);
        assert_eq!(form.fields[0].submitted_value(), "");
    }
}

#[test]
fn moving_around_or_submitting_keeps_a_saved_secret() {
    let mut form =
        FormDialog::new("t", vec![FormField::text("a", "A", ""), FormField::saved_secret("password", "Password")]);
    press(&mut form, KeyCode::Tab);
    press(&mut form, KeyCode::Left);
    press(&mut form, KeyCode::Right);
    press(&mut form, KeyCode::Home);
    press(&mut form, KeyCode::End);
    press(&mut form, KeyCode::BackTab);
    press(&mut form, KeyCode::Tab);

    match press(&mut form, KeyCode::Enter) {
        FormOutcome::Submitted(values) => assert_eq!(values[1], ("password", KEPT_SECRET.to_string())),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn typing_can_never_produce_the_kept_marker() {
    let mut form = FormDialog::new("t", vec![FormField::masked("password", "Password", "")]);
    for character in KEPT_SECRET.chars().filter(|character| !character.is_control()) {
        press(&mut form, KeyCode::Char(character));
    }
    assert_ne!(form.fields[0].submitted_value(), KEPT_SECRET);
    assert!(KEPT_SECRET.chars().any(char::is_control));
}

#[test]
fn a_saved_secret_shows_that_it_is_saved_without_a_value() {
    let form = FormDialog::new("t", vec![FormField::saved_secret("password", "Password")]);
    let screen = screen_of(&form);
    assert!(screen.contains("(saved)"), "{screen}");
    assert!(screen.contains("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"), "{screen}");
}

#[test]
fn an_edited_saved_secret_is_shown_masked() {
    let mut form = FormDialog::new("t", vec![FormField::saved_secret("password", "Password")]);
    press(&mut form, KeyCode::Char('s'));
    press(&mut form, KeyCode::Char('3'));
    let screen = screen_of(&form);
    assert!(!screen.contains("(saved)") && !screen.contains("s3"), "{screen}");
    assert!(screen.contains("**"), "{screen}");
}

#[test]
fn a_hint_line_is_rendered_and_absent_by_default() {
    let mut form = FormDialog::new("t", vec![FormField::text("a", "A", "")]);
    assert!(form.hint.is_none());
    let plain = screen_of(&form);
    form.hint = Some("Passwords are not saved".into());
    let hinted = screen_of(&form);
    assert!(!plain.contains("Passwords are not saved"));
    assert!(hinted.contains("Passwords are not saved"), "{hinted}");
}

#[test]
fn a_submitted_form_never_prints_its_values() {
    let outcome = FormOutcome::Submitted(vec![("password", "hunter2".to_string()), ("host", "h".to_string())]);

    let printed = format!("{outcome:?}");

    assert!(!printed.contains("hunter2"), "{printed}");
    assert!(printed.contains("password") && printed.contains("host"), "{printed}");
}
