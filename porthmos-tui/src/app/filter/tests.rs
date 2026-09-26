use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use porthmos_core::{Command, Location};

use crate::{
    app::testing::{entry, test_app},
    widgets::panel_view::Row,
};

fn press(test: &mut crate::app::testing::TestApp, code: KeyCode) {
    test.app.apply_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn type_text(test: &mut crate::app::testing::TestApp, text: &str) {
    for character in text.chars() {
        press(test, KeyCode::Char(character));
    }
}

fn filtered_app() -> crate::app::testing::TestApp {
    let mut test = test_app(Path::new("/d"));
    let dir = Path::new("/d");
    test.list_local(vec![
        entry(dir, "porthmos", true),
        entry(dir, "Report.pdf", false),
        entry(dir, "notes.txt", false),
    ]);
    test
}

fn visible(test: &crate::app::testing::TestApp) -> Vec<String> {
    test.app
        .local
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Entry(entry) => Some(entry.name.clone()),
            Row::Parent => None,
        })
        .collect()
}

#[test]
fn slash_opens_the_filter_line_and_typing_narrows_the_panel() {
    let mut test = filtered_app();

    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "port");

    assert!(test.app.local.editing_filter());
    assert_eq!(visible(&test), vec!["porthmos".to_string(), "Report.pdf".to_string()]);
}

#[test]
fn letters_in_the_filter_line_are_text_not_shortcuts() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));

    type_text(&mut test, " /q");

    assert_eq!(test.app.local.filter(), Some(" /q"));
    assert!(!test.app.should_quit);
}

#[test]
fn backspace_edits_and_enter_keeps_the_filter() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "pdfx");

    press(&mut test, KeyCode::Backspace);
    press(&mut test, KeyCode::Enter);

    assert!(!test.app.local.editing_filter());
    assert_eq!(visible(&test), vec!["Report.pdf".to_string()]);
}

#[test]
fn enter_on_a_folder_while_typing_does_not_open_it() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "porth");
    assert_eq!(test.app.local.current_entry_name(), Some("porthmos"));

    press(&mut test, KeyCode::Enter);

    assert_eq!(test.app.local.path(), Path::new("/d"));
    assert!(test.sent().is_empty());
}

#[test]
fn arrows_move_the_cursor_while_typing() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "o");
    let before = test.app.local.cursor;

    press(&mut test, KeyCode::Down);

    assert_eq!(test.app.local.cursor, before + 1);
    assert!(test.app.local.editing_filter());
}

#[test]
fn esc_while_typing_clears_the_filter() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "port");

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.app.local.filter(), None);
    assert!(!test.app.local.editing_filter());
    assert_eq!(visible(&test).len(), 3);
}

#[test]
fn esc_while_browsing_clears_the_filter_before_anything_else() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "port");
    press(&mut test, KeyCode::Enter);
    test.app.notifications.push(porthmos_core::Severity::Error, "boom");

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.app.local.filter(), None);
    assert_eq!(test.notification().as_deref(), Some("boom"));
}

#[test]
fn slash_reopens_the_line_with_the_current_text() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "po");
    press(&mut test, KeyCode::Enter);

    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "rt");

    assert_eq!(test.app.local.filter(), Some("port"));
}

#[test]
fn a_rebound_filter_key_opens_the_line() {
    let mut test = filtered_app();
    let mut overrides = std::collections::HashMap::new();
    overrides.insert("filter".to_string(), "ctrl+g".to_string());
    let (bindings, _) = crate::input::KeyBindings::from_overrides(&overrides);
    test.app.key_bindings = bindings;

    test.app.apply_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert!(test.app.local.editing_filter());
}

#[test]
fn a_second_esc_dismisses_the_error_after_the_filter_is_gone() {
    let mut test = filtered_app();
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "port");
    press(&mut test, KeyCode::Enter);
    test.app.notifications.push(porthmos_core::Severity::Error, "boom");

    press(&mut test, KeyCode::Esc);
    press(&mut test, KeyCode::Esc);

    assert_eq!(test.notification(), None);
}

#[test]
fn a_shortcut_key_while_typing_keeps_the_filter_and_does_its_job() {
    let mut test = filtered_app();
    let session = test.connect(3, "srv");
    test.list_remote(session, "/srv", Vec::new());
    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "pdf");

    press(&mut test, KeyCode::F(5));

    assert!(!test.app.local.editing_filter());
    assert_eq!(test.app.local.filter(), Some("pdf"));
    assert_eq!(
        test.sent(),
        vec![Command::Copy {
            from: Location::Local,
            entries: vec![entry(Path::new("/d"), "Report.pdf", false)],
            to: Location::Session(session),
            dest_dir: Path::new("/srv").to_path_buf(),
        }]
    );
}

#[test]
fn the_remote_panel_filters_too() {
    let mut test = filtered_app();
    let session = test.connect(3, "srv");
    let dir = Path::new("/srv");
    test.list_remote(session, "/srv", vec![entry(dir, "backup.tar", false), entry(dir, "www", true)]);
    test.app.active_panel = crate::widgets::panel_view::ActivePanel::Remote;

    press(&mut test, KeyCode::Char('/'));
    type_text(&mut test, "ww");

    let panel = &test.app.sessions.active().unwrap().panel;
    assert!(panel.editing_filter());
    assert_eq!(panel.filter(), Some("ww"));
    assert_eq!(panel.current_entry_name(), Some("www"));
    assert_eq!(test.app.local.filter(), None);
}
