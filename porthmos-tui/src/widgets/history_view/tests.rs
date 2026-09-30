use chrono::{TimeZone, Utc};
use porthmos_core::transfer::Direction;
use ratatui::{Terminal, backend::TestBackend, style::Color};

use super::*;

fn entry(label: &str, connection: &str, result: HistoryResult) -> HistoryEntry {
    HistoryEntry {
        finished_at: Utc.with_ymd_and_hms(2026, 9, 30, 8, 12, 44).unwrap(),
        connection: connection.to_string(),
        direction: Direction::Upload,
        label: label.to_string(),
        local_path: format!("/home/me/{label}"),
        remote_path: format!("/srv/{label}"),
        files_done: 1,
        files_total: 2,
        bytes: 2048,
        result,
        failed_count: 0,
        failed_files: Vec::new(),
    }
}

fn view_of(entries: Vec<HistoryEntry>) -> HistoryView {
    let mut view = HistoryView::new();
    view.replace(entries);
    view
}

fn three() -> HistoryView {
    view_of(vec![
        entry("newest.txt", "prod", HistoryResult::Done),
        entry("middle.txt", "staging", HistoryResult::PartlyFailed { failed: 1 }),
        entry("oldest.txt", "prod", HistoryResult::Failed),
    ])
}

fn labels(view: &HistoryView) -> Vec<&str> {
    view.rows().map(|entry| entry.label.as_str()).collect()
}

fn render(view: &HistoryView, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| render_history(frame, frame.area(), view)).unwrap();
    terminal
}

fn screen(terminal: &Terminal<TestBackend>) -> String {
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn rows_keep_the_order_they_were_given() {
    assert_eq!(labels(&three()), ["newest.txt", "middle.txt", "oldest.txt"]);
}

#[test]
fn the_selection_starts_on_the_first_row_and_moves_within_bounds() {
    let mut view = three();
    assert_eq!(view.selected().unwrap().label, "newest.txt");

    view.move_cursor(1);
    assert_eq!(view.selected().unwrap().label, "middle.txt");
    view.move_cursor(10);
    assert_eq!(view.selected().unwrap().label, "oldest.txt");
    view.move_cursor(-10);
    assert_eq!(view.selected().unwrap().label, "newest.txt");
}

#[test]
fn an_empty_view_selects_nothing_and_ignores_movement() {
    let mut view = HistoryView::new();

    view.move_cursor(1);

    assert!(view.selected().is_none());
    assert_eq!(view.total(), 0);
}

#[test]
fn typing_a_filter_narrows_the_rows_and_erasing_widens_them() {
    let mut view = three();
    view.start_filter();

    for character in "prod".chars() {
        view.type_filter(character);
    }
    assert_eq!(labels(&view), ["newest.txt", "oldest.txt"]);
    assert_eq!((view.matched(), view.total()), (2, 3));

    for _ in 0..4 {
        view.erase_filter();
    }
    assert_eq!(labels(&view).len(), 3);
    assert_eq!(view.filter(), None);
}

#[test]
fn the_word_failed_finds_partly_failed_entries_too() {
    let mut view = three();
    view.start_filter();

    for character in "failed".chars() {
        view.type_filter(character);
    }

    assert_eq!(labels(&view), ["middle.txt", "oldest.txt"]);
}

#[test]
fn a_blank_filter_shows_everything() {
    let mut view = three();
    view.start_filter();

    view.type_filter(' ');

    assert_eq!(labels(&view).len(), 3);
}

#[test]
fn clearing_the_filter_stops_editing_and_shows_everything() {
    let mut view = three();
    view.start_filter();
    view.type_filter('z');
    assert_eq!(labels(&view).len(), 0);

    view.clear_filter();

    assert!(!view.editing_filter());
    assert_eq!(view.filter(), None);
    assert_eq!(labels(&view).len(), 3);
}

#[test]
fn finishing_the_filter_keeps_it_applied() {
    let mut view = three();
    view.start_filter();
    for character in "mid".chars() {
        view.type_filter(character);
    }
    view.finish_filter();

    assert!(!view.editing_filter());
    assert_eq!(view.filter(), Some("mid"));
    assert_eq!(labels(&view), ["middle.txt"]);
}

#[test]
fn replacing_the_entries_keeps_the_filter_and_a_valid_cursor() {
    let mut view = three();
    view.start_filter();
    view.type_filter('t');
    view.finish_filter();
    view.move_cursor(2);
    assert_eq!(view.cursor(), 2);

    view.replace(vec![entry("only.txt", "prod", HistoryResult::Done)]);

    assert_eq!(view.selected().unwrap().label, "only.txt");
    assert_eq!(view.cursor(), 0);
    assert_eq!(view.filter(), Some("t"));
}

#[test]
fn replacing_with_nothing_leaves_no_selection() {
    let mut view = three();
    view.move_cursor(2);

    view.replace(Vec::new());

    assert!(view.selected().is_none());
}

#[test]
fn go_to_top_selects_the_first_row() {
    let mut view = three();
    view.move_cursor(2);

    view.go_to_top();

    assert_eq!(view.cursor(), 0);
}

#[test]
fn markers_tell_failures_apart() {
    assert_eq!(result_marker(&HistoryResult::Failed), '\u{2717}');
    assert_eq!(result_marker(&HistoryResult::PartlyFailed { failed: 1 }), '!');
    assert_eq!(result_marker(&HistoryResult::Done), ' ');
    assert_eq!(result_marker(&HistoryResult::Cancelled), ' ');
    assert_eq!(result_marker(&HistoryResult::Interrupted), ' ');
}

#[test]
fn rows_show_direction_label_connection_counts_and_size() {
    let content = screen(&render(&three(), 100, 8));

    assert!(content.contains("History (3)"), "{content}");
    assert!(content.contains("\u{2191} newest.txt"), "{content}");
    assert!(content.contains("prod"), "{content}");
    assert!(content.contains("1/2"), "{content}");
    assert!(content.contains("2.0 KB") || content.contains("2 KB") || content.contains("2.0K"), "{content}");
    assert!(content.contains('\u{2717}'), "{content}");
}

#[test]
fn failed_rows_are_red_and_partly_failed_rows_yellow() {
    let view = view_of(vec![
        entry("a", "prod", HistoryResult::Done),
        entry("b", "prod", HistoryResult::PartlyFailed { failed: 1 }),
        entry("c", "prod", HistoryResult::Failed),
    ]);
    let terminal = render(&view, 100, 8);
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(5, 2)].style().fg, Some(Color::Yellow));
    assert_eq!(buffer[(5, 3)].style().fg, Some(Color::Red));
}

#[test]
fn cancelled_and_interrupted_rows_are_dimmed() {
    let view = view_of(vec![
        entry("a", "prod", HistoryResult::Done),
        entry("b", "prod", HistoryResult::Cancelled),
        entry("c", "prod", HistoryResult::Interrupted),
    ]);
    let terminal = render(&view, 100, 8);
    let buffer = terminal.backend().buffer();

    assert!(buffer[(5, 2)].style().add_modifier.contains(ratatui::style::Modifier::DIM));
    assert!(buffer[(5, 3)].style().add_modifier.contains(ratatui::style::Modifier::DIM));
    assert!(!buffer[(5, 1)].style().add_modifier.contains(ratatui::style::Modifier::DIM));
}

#[test]
fn an_empty_history_says_so() {
    let content = screen(&render(&HistoryView::new(), 60, 6));

    assert!(content.contains("No transfers yet"), "{content}");
}

#[test]
fn a_filter_with_no_match_says_so_and_shows_the_filter_line() {
    let mut view = three();
    view.start_filter();
    view.type_filter('z');

    let content = screen(&render(&view, 60, 6));

    assert!(content.contains("No matches"), "{content}");
    assert!(content.contains("/z"), "{content}");
    assert!(content.contains("(0 of 3)"), "{content}");
}

#[test]
fn a_narrow_screen_does_not_panic() {
    for width in [4, 10, 24] {
        render(&three(), width, 6);
    }
}

#[test]
fn a_moderately_narrow_screen_still_shows_the_label() {
    let content = screen(&render(&three(), 40, 6));

    assert!(content.contains("newest"), "{content}");
}

#[test]
fn a_new_entry_arriving_keeps_the_selection_on_the_same_transfer() {
    let mut view = three();
    view.move_cursor(1);
    assert_eq!(view.selected().unwrap().label, "middle.txt");

    view.replace(vec![
        entry("brand-new.txt", "prod", HistoryResult::Done),
        entry("newest.txt", "prod", HistoryResult::Done),
        entry("middle.txt", "staging", HistoryResult::PartlyFailed { failed: 1 }),
        entry("oldest.txt", "prod", HistoryResult::Failed),
    ]);

    assert_eq!(view.selected().unwrap().label, "middle.txt");
    assert_eq!(view.cursor(), 2);
}

#[test]
fn a_label_with_a_newline_stays_on_one_row() {
    let view = view_of(vec![entry("a\nb", "prod", HistoryResult::Done)]);
    let terminal = render(&view, 80, 6);
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> =
        (0..6).map(|y| (0..80).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>()).collect();

    assert_eq!(rows.iter().filter(|row| row.contains("a?b")).count(), 1, "{rows:?}");
}

#[test]
fn printable_replaces_control_characters_only() {
    assert_eq!(printable("a\nb\u{1b}[0m\tc \u{fc}"), "a?b?[0m?c \u{fc}");
}
