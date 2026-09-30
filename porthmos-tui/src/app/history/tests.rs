use std::path::Path;

use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use porthmos_core::history::{HistoryEntry, HistoryResult};
use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::app::testing::{TestApp, test_app};

fn entry(label: &str, result: HistoryResult) -> HistoryEntry {
    HistoryEntry {
        finished_at: Utc.with_ymd_and_hms(2026, 9, 30, 8, 12, 44).unwrap(),
        connection: "prod".to_string(),
        direction: Direction::Upload,
        label: label.to_string(),
        local_path: format!("/home/me/{label}"),
        remote_path: format!("/srv/{label}"),
        files_done: 1,
        files_total: 1,
        bytes: 10,
        result,
        failed_count: 0,
        failed_files: Vec::new(),
    }
}

fn app_with_history(entries: Vec<HistoryEntry>) -> TestApp {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenHistory);
    test.sent();
    test.app.apply_core_event(Event::History(entries));
    test
}

fn press(test: &mut TestApp, code: KeyCode) {
    test.app.apply_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn three() -> Vec<HistoryEntry> {
    vec![
        entry("newest", HistoryResult::Done),
        entry("middle", HistoryResult::Failed),
        entry("oldest", HistoryResult::Done),
    ]
}

#[test]
fn opening_the_history_asks_the_core_for_it_and_shows_the_screen() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_action(Action::OpenHistory);

    assert_eq!(test.app.screen, Screen::History);
    assert_eq!(test.sent(), vec![Command::ListHistory]);
}

#[test]
fn ctrl_y_opens_the_history_from_the_file_screen() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));

    assert_eq!(test.app.screen, Screen::History);
}

#[test]
fn a_history_event_fills_the_view_with_the_newest_selected() {
    let test = app_with_history(three());

    assert_eq!(test.app.history.total(), 3);
    assert_eq!(test.app.history.selected().unwrap().label, "newest");
}

#[test]
fn up_and_down_move_the_selection_within_the_list() {
    let mut test = app_with_history(three());

    press(&mut test, KeyCode::Down);
    assert_eq!(test.app.history.selected().unwrap().label, "middle");
    press(&mut test, KeyCode::Down);
    press(&mut test, KeyCode::Down);
    assert_eq!(test.app.history.selected().unwrap().label, "oldest");
    press(&mut test, KeyCode::Up);
    assert_eq!(test.app.history.selected().unwrap().label, "middle");
}

#[test]
fn reopening_the_screen_returns_to_the_newest_row() {
    let mut test = app_with_history(three());
    press(&mut test, KeyCode::Down);
    press(&mut test, KeyCode::Esc);

    test.app.apply_action(Action::OpenHistory);

    assert_eq!(test.app.history.selected().unwrap().label, "newest");
}

#[test]
fn escape_leaves_the_screen() {
    let mut test = app_with_history(three());

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.app.screen, Screen::Files);
}

#[test]
fn slash_opens_the_filter_and_typing_narrows_the_rows() {
    let mut test = app_with_history(three());

    press(&mut test, KeyCode::Char('/'));
    for character in "mid".chars() {
        press(&mut test, KeyCode::Char(character));
    }

    assert!(test.app.history.editing_filter());
    assert_eq!(test.app.history.matched(), 1);
    assert_eq!(test.app.history.selected().unwrap().label, "middle");
}

#[test]
fn while_typing_a_filter_up_and_down_still_move_the_selection() {
    let mut test = app_with_history(three());

    press(&mut test, KeyCode::Char('/'));
    press(&mut test, KeyCode::Down);

    assert!(test.app.history.editing_filter());
    assert_eq!(test.app.history.selected().unwrap().label, "middle");
}

#[test]
fn escape_clears_the_filter_before_it_leaves_the_screen() {
    let mut test = app_with_history(three());
    press(&mut test, KeyCode::Char('/'));
    press(&mut test, KeyCode::Char('m'));
    press(&mut test, KeyCode::Enter);
    assert_eq!(test.app.history.filter(), Some("m"));

    press(&mut test, KeyCode::Esc);
    assert_eq!(test.app.history.filter(), None);
    assert_eq!(test.app.screen, Screen::History);

    press(&mut test, KeyCode::Esc);
    assert_eq!(test.app.screen, Screen::Files);
}

#[test]
fn a_refresh_that_shrinks_the_list_keeps_the_selection_valid() {
    let mut test = app_with_history(three());
    press(&mut test, KeyCode::Down);
    press(&mut test, KeyCode::Down);

    test.app.apply_core_event(Event::History(vec![entry("only", HistoryResult::Done)]));

    assert_eq!(test.app.history.selected().unwrap().label, "only");
}

#[test]
fn a_refresh_with_a_filter_active_keeps_the_filter() {
    let mut test = app_with_history(three());
    press(&mut test, KeyCode::Char('/'));
    for character in "oth".chars() {
        press(&mut test, KeyCode::Char(character));
    }
    press(&mut test, KeyCode::Enter);

    test.app.apply_core_event(Event::History(vec![
        entry("another", HistoryResult::Done),
        entry("nothing", HistoryResult::Done),
        entry("x", HistoryResult::Done),
    ]));

    assert_eq!(test.app.history.matched(), 2);
}

#[test]
fn the_screen_draws_its_title_and_rows() {
    let test = app_with_history(three());
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();

    terminal.draw(|frame| test.app.render(frame)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(content.contains("History (3)"), "{content}");
    assert!(content.contains("newest"), "{content}");
}

fn failed_entry(stored: usize, count: usize) -> HistoryEntry {
    let mut failed = entry("docs", HistoryResult::PartlyFailed { failed: count });
    failed.failed_count = count;
    failed.failed_files = (0..stored).map(|index| format!("bad{index}.txt")).collect();
    failed
}

fn dialog_message(test: &TestApp) -> String {
    match &test.app.dialog {
        Some(Dialog::Message(message)) => message.message.clone(),
        _ => panic!("expected the details dialog"),
    }
}

#[test]
fn enter_opens_the_details_of_the_selected_entry() {
    let mut test = app_with_history(three());
    press(&mut test, KeyCode::Down);

    press(&mut test, KeyCode::Enter);

    let text = dialog_message(&test);
    assert!(text.contains("Connection: prod"), "{text}");
    match &test.app.dialog {
        Some(Dialog::Message(message)) => assert!(message.title.contains("middle"), "{}", message.title),
        _ => panic!("expected the details dialog"),
    }
}

#[test]
fn enter_on_an_empty_history_does_nothing() {
    let mut test = app_with_history(Vec::new());

    press(&mut test, KeyCode::Enter);

    assert!(test.app.dialog.is_none());
}

#[test]
fn enter_closes_the_details() {
    let mut test = app_with_history(three());
    press(&mut test, KeyCode::Enter);

    press(&mut test, KeyCode::Enter);

    assert!(test.app.dialog.is_none());
    assert_eq!(test.app.screen, Screen::History);
}

#[test]
fn a_done_entry_has_no_failure_section_and_no_log_pointer() {
    let text = details_text(&entry("a.txt", HistoryResult::Done));

    assert!(!text.contains("Failed files"), "{text}");
    assert!(!text.contains("porthmos.log"), "{text}");
    assert!(text.contains("Result:     done"), "{text}");
    assert!(text.contains("Local:      /home/me/a.txt"), "{text}");
    assert!(text.contains("Remote:     /srv/a.txt"), "{text}");
}

#[test]
fn a_failed_scan_without_paths_omits_the_path_lines_but_points_at_the_log() {
    let mut failed = entry("photos", HistoryResult::Failed);
    failed.local_path = String::new();
    failed.remote_path = String::new();

    let text = details_text(&failed);

    assert!(!text.contains("Local:"), "{text}");
    assert!(!text.contains("Remote:"), "{text}");
    assert!(text.contains("porthmos.log"), "{text}");
}

#[test]
fn failed_names_are_listed_in_full_up_to_ten() {
    let text = details_text(&failed_entry(3, 3));

    assert!(text.contains("Result:     partly failed (3 failed)"), "{text}");
    assert!(text.contains("- bad0.txt") && text.contains("- bad2.txt"), "{text}");
    assert!(!text.contains("more"), "{text}");
    assert!(text.contains("porthmos.log"), "{text}");

    let ten = details_text(&failed_entry(10, 10));
    assert!(ten.contains("- bad9.txt"), "{ten}");
    assert!(!ten.contains("more"), "{ten}");
}

#[test]
fn eleven_failed_names_show_ten_and_a_count_of_the_rest() {
    let text = details_text(&failed_entry(11, 11));

    assert!(text.contains("- bad9.txt"), "{text}");
    assert!(!text.contains("bad10.txt"), "{text}");
    assert!(text.contains("and 1 more\u{2026}"), "{text}");
}

#[test]
fn a_capped_name_list_still_reports_the_true_count() {
    let text = details_text(&failed_entry(100, 250));

    assert!(text.contains("and 240 more\u{2026}"), "{text}");
}

#[test]
fn control_characters_in_names_never_reach_the_dialog() {
    let mut hostile = entry("evil\u{1b}[31m.txt", HistoryResult::Failed);
    hostile.connection = "bad\rhost".to_string();
    hostile.failed_files = vec!["x\u{1b}]0;title\u{7}y".to_string()];
    hostile.failed_count = 1;

    let text = details_text(&hostile);

    let title = details_title(&hostile);

    assert!(!text.chars().any(|character| character.is_control() && character != '\n'), "{text:?}");
    assert!(!title.chars().any(char::is_control), "{title:?}");
    assert!(title.contains("evil"), "{title}");
}

#[test]
fn f8_asks_before_clearing_and_only_a_yes_clears() {
    let mut test = app_with_history(three());

    test.app.apply_action(Action::Delete);
    assert!(matches!(test.app.dialog, Some(Dialog::Confirm(_))));
    press(&mut test, KeyCode::Char('n'));
    assert!(test.sent().is_empty());

    test.app.apply_action(Action::Delete);
    press(&mut test, KeyCode::Char('y'));
    assert_eq!(test.sent(), vec![Command::ClearHistory]);
    assert!(test.app.dialog.is_none());
}

#[test]
fn f8_on_an_empty_history_asks_nothing() {
    let mut test = app_with_history(Vec::new());

    test.app.apply_action(Action::Delete);

    assert!(test.app.dialog.is_none());
}

#[test]
fn clearing_history_sends_only_the_clear_command() {
    let mut test = app_with_history(three());

    test.app.apply_action(Action::Delete);
    press(&mut test, KeyCode::Char('y'));

    assert_eq!(test.sent(), vec![Command::ClearHistory]);
}

#[test]
fn an_interrupted_entry_with_failed_files_points_at_the_log() {
    let mut interrupted = entry("docs", HistoryResult::Interrupted);
    interrupted.failed_files = vec!["a.txt".to_string()];
    interrupted.failed_count = 1;

    let text = details_text(&interrupted);

    assert!(text.contains("- a.txt"), "{text}");
    assert!(text.contains("porthmos.log"), "{text}");
}

#[test]
fn the_worst_case_details_fit_an_80_by_24_terminal() {
    let mut worst = failed_entry(100, 250);
    worst.local_path = "/home/someone/projects/client-work/2026/september/reports".to_string();
    worst.remote_path = "/srv/backups/client-work/2026/september/reports/final".to_string();
    let dialog = MessageDialog::new(details_title(&worst), details_text(&worst));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

    terminal.draw(|frame| crate::widgets::dialog::message::render_message(frame, frame.area(), &dialog)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(content.contains("and 240 more"), "{content}");
    assert!(content.contains("porthmos.log"), "{content}");
    assert!(content.contains("[Enter] OK"), "{content}");
}
