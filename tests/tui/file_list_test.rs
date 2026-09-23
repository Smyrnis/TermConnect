use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::fs;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

use crate::filesystem::Entry;
use crate::tui::panels::{PanelState, Row};

use super::*;

const THREE_CHARS_SIX_COLUMNS: &str = "\u{65e5}\u{672c}\u{8a9e}";
#[test]
fn renders_entry_names() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("readme.txt"), b"content").unwrap();
    let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    let backend = TestBackend::new(30, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_file_list(frame, frame.area(), &panel, true)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("readme.txt"));
}

#[test]
fn format_size_shows_raw_bytes_under_1024() {
    assert_eq!(format_size(0, false), "0");
    assert_eq!(format_size(999, false), "999");
}

#[test]
fn format_size_uses_kilobyte_units_above_1024() {
    assert_eq!(format_size(1024, false), "1.0K");
    assert_eq!(format_size(1536, false), "1.5K");
}

#[test]
fn format_size_uses_megabyte_and_gigabyte_units() {
    assert_eq!(format_size(2 * 1024 * 1024, false), "2.0M");
    assert_eq!(format_size(3 * 1024 * 1024 * 1024, false), "3.0G");
}

#[test]
fn format_size_shows_an_em_dash_for_directories() {
    assert_eq!(format_size(4096, true), "\u{2014}");
}

#[test]
fn format_permissions_renders_rwx_triplets() {
    assert_eq!(format_permissions(Some(0o755)), "rwxr-xr-x");
    assert_eq!(format_permissions(Some(0o640)), "rw-r-----");
}

#[test]
fn format_permissions_renders_dashes_when_unknown() {
    assert_eq!(format_permissions(None), "---------");
}

#[test]
fn truncate_name_measures_display_width_not_char_count() {
    let name = "\u{65e5}\u{672c}\u{8a9e}\u{30d5}\u{30a1}\u{30a4}\u{30eb}";

    let truncated = truncate_name(name, 6);

    assert!(UnicodeWidthStr::width(truncated.as_str()) <= 6);
    assert!(truncated.ends_with('\u{2026}'));
}

#[test]
fn truncate_name_leaves_a_wide_name_that_already_fits_untouched() {
    let name = THREE_CHARS_SIX_COLUMNS;

    assert_eq!(truncate_name(name, 10), name);
}

#[test]
fn row_label_pads_a_wide_name_to_the_correct_display_width() {
    let entry = Entry {
        name: THREE_CHARS_SIX_COLUMNS.to_string(),
        path: PathBuf::from("/tmp/entry"),
        is_dir: false,
        size: 0,
        permissions: None,
    };
    let columns = Columns { name_width: 10, show_size: false, show_permissions: false };

    let label = row_label(&Row::Entry(entry), false, columns);

    assert_eq!(UnicodeWidthStr::width(label.as_str()), 2 + columns.name_width);
}

#[test]
fn wide_panel_shows_size_and_permissions_columns() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("readme.txt"), b"hello").unwrap();
    let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    let backend = TestBackend::new(60, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_file_list(frame, frame.area(), &panel, true)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains('5'));
    assert!(content.contains('r') || content.contains('-'));
}

#[test]
fn narrow_panel_hides_size_and_permissions_columns() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), vec![0u8; 123_456]).unwrap();
    let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    let backend = TestBackend::new(20, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_file_list(frame, frame.area(), &panel, true)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("a.txt"));
    assert!(!content.contains("120.6K"));
}
