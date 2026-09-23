use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::fs;

#[test]
fn toggle_switches_between_local_and_remote() {
    let mut panel = ActivePanel::Local;
    panel.toggle();
    assert_eq!(panel, ActivePanel::Remote);
    panel.toggle();
    assert_eq!(panel, ActivePanel::Local);
}

#[test]
fn from_listing_builds_rows_from_provided_entries() {
    let entries = vec![Entry {
        name: "remote_dir".to_string(),
        path: PathBuf::from("/home/user/remote_dir"),
        is_dir: true,
        size: 0,
        permissions: None,
    }];

    let panel = PanelState::from_listing(PathBuf::from("/home/user"), entries);

    assert_eq!(panel.path(), Path::new("/home/user"));
    assert_eq!(panel.rows().len(), 2);
    assert_eq!(panel.rows()[0], Row::Parent);
}

#[test]
fn from_listing_at_root_has_no_parent_row() {
    let panel = PanelState::from_listing(PathBuf::from("/"), Vec::new());

    assert!(panel.rows().is_empty());
}

#[test]
fn target_path_for_open_resolves_parent_and_directory_targets() {
    let entries = vec![Entry {
        name: "child".to_string(),
        path: PathBuf::from("/home/user/child"),
        is_dir: true,
        size: 0,
        permissions: None,
    }];
    let mut panel = PanelState::from_listing(PathBuf::from("/home/user"), entries);

    assert_eq!(panel.target_path_for_open(), Some(PathBuf::from("/home")));

    panel.cursor = 1;
    assert_eq!(panel.target_path_for_open(), Some(PathBuf::from("/home/user/child")));
}

#[test]
fn new_panel_lists_the_given_directory() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.txt"), b"content").unwrap();

    let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    assert_eq!(panel.rows().len(), 2);
    assert_eq!(panel.rows()[0], Row::Parent);
}

#[test]
fn move_cursor_clamps_within_bounds() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.txt"), b"content").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    panel.move_cursor(-5);
    assert_eq!(panel.cursor, 0);

    panel.move_cursor(5);
    assert_eq!(panel.cursor, panel.rows().len() - 1);
}

#[test]
fn navigate_to_changes_the_path_and_refreshes() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    panel.navigate_to(child.clone()).unwrap();

    assert_eq!(panel.path(), child);
}

#[test]
fn open_selected_navigates_into_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("child")).unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    panel.cursor = panel.rows().len() - 1;

    panel.open_selected().unwrap();

    assert_eq!(panel.path(), dir.path().join("child"));
}

#[test]
fn open_selected_on_parent_row_navigates_up() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    let mut panel = PanelState::new(child.clone()).unwrap();
    assert_eq!(panel.rows()[0], Row::Parent);

    panel.open_selected().unwrap();

    assert_eq!(panel.path(), dir.path());
}

#[test]
fn toggle_selection_adds_and_removes_the_current_entry() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.txt"), b"content").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    panel.cursor = panel.rows().len() - 1;

    panel.toggle_selection();
    assert_eq!(panel.selected.len(), 1);

    panel.toggle_selection();
    assert_eq!(panel.selected.len(), 0);
}

#[test]
fn target_entries_returns_the_cursor_entry_without_a_selection() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("only.txt"), b"content").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    panel.cursor = panel.rows().len() - 1;

    let entries = panel.target_entries();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "only.txt");
}

#[test]
fn target_entries_returns_all_selected_entries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), b"content").unwrap();
    fs::write(dir.path().join("b.txt"), b"content").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    panel.cursor = 1;
    panel.toggle_selection();
    panel.cursor = 2;
    panel.toggle_selection();

    let mut names: Vec<String> = panel.target_entries().into_iter().map(|entry| entry.name).collect();
    names.sort();

    assert_eq!(names, vec!["a.txt".to_string(), "b.txt".to_string()]);
}

#[test]
fn create_directory_adds_a_new_row_after_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    panel.create_directory("new_dir").unwrap();

    assert!(dir.path().join("new_dir").is_dir());
    assert!(panel.rows().iter().any(|row| matches!(row, Row::Entry(entry) if entry.name == "new_dir")));
}

#[test]
fn rename_current_renames_the_entry_under_the_cursor() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("old.txt"), b"content").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    panel.cursor = panel.rows().len() - 1;

    panel.rename_current("new.txt").unwrap();

    assert!(!dir.path().join("old.txt").exists());
    assert!(dir.path().join("new.txt").exists());
}

#[test]
fn delete_targets_removes_selected_entries() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), b"content").unwrap();
    fs::write(dir.path().join("b.txt"), b"content").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    panel.cursor = 1;
    panel.toggle_selection();
    panel.cursor = 2;
    panel.toggle_selection();

    panel.delete_targets().unwrap();

    assert!(!dir.path().join("a.txt").exists());
    assert!(!dir.path().join("b.txt").exists());
}

#[test]
fn delete_targets_falls_back_to_cursor_entry_without_selection() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("only.txt"), b"content").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    panel.cursor = panel.rows().len() - 1;

    panel.delete_targets().unwrap();

    assert!(!dir.path().join("only.txt").exists());
}

#[test]
fn render_panel_draws_the_given_title() {
    let dir = tempfile::tempdir().unwrap();
    let panel = PanelState::new(dir.path().to_path_buf()).unwrap();
    let backend = TestBackend::new(20, 5);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            render_panel(frame, Rect::new(0, 0, 20, 5), "LOCAL", false, &panel);
        })
        .unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("LOCAL"));
}

#[test]
fn hidden_entries_are_excluded_by_default() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".hidden"), b"x").unwrap();
    fs::write(dir.path().join("visible.txt"), b"x").unwrap();

    let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    let names: Vec<String> = panel
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Entry(e) => Some(e.name.clone()),
            Row::Parent => None,
        })
        .collect();
    assert_eq!(names, vec!["visible.txt".to_string()]);
}

#[test]
fn toggle_hidden_reveals_dotfiles_without_touching_the_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".hidden"), b"x").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    panel.toggle_hidden();

    let names: Vec<String> = panel
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Entry(e) => Some(e.name.clone()),
            Row::Parent => None,
        })
        .collect();
    assert_eq!(names, vec![".hidden".to_string()]);

    panel.toggle_hidden();
    assert_eq!(panel.rows().len(), 1);
}

#[test]
fn cycle_sort_reorders_rows_by_size_when_advanced_twice() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("big.txt"), vec![0u8; 100]).unwrap();
    fs::write(dir.path().join("small.txt"), vec![0u8; 1]).unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    panel.cycle_sort();
    panel.cycle_sort();

    let names: Vec<String> = panel
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Entry(e) => Some(e.name.clone()),
            Row::Parent => None,
        })
        .collect();
    assert_eq!(names, vec!["small.txt".to_string(), "big.txt".to_string()]);
}

#[test]
fn set_sort_spec_and_set_show_hidden_apply_immediately() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(".hidden"), b"x").unwrap();
    let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();

    panel.set_show_hidden(true);
    assert!(panel.show_hidden());
    assert_eq!(panel.rows().len(), 2);

    panel.set_sort_spec(crate::tui::sort::SortSpec {
        key: crate::tui::sort::SortKey::Size,
        order: crate::tui::sort::SortOrder::Descending,
    });
    assert_eq!(
        panel.sort_spec(),
        crate::tui::sort::SortSpec {
            key: crate::tui::sort::SortKey::Size,
            order: crate::tui::sort::SortOrder::Descending
        }
    );
}
