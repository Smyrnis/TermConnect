use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::fs;

use crate::tui::panels::PanelState;

use super::*;

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
fn wide_panel_shows_size_and_permissions_columns() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("readme.txt"), b"hello").unwrap();
    let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    let backend = TestBackend::new(60, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_file_list(frame, frame.area(), &panel, true)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains('5')); // the 5-byte size
    assert!(content.contains('r') || content.contains('-')); // permissions column present
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
    assert!(!content.contains("120.6K")); // the size string must not appear
}
