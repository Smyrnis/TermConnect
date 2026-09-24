use ratatui::{Terminal, backend::TestBackend};

use super::*;

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

    let panel = PanelView::from_listing(PathBuf::from("/home/user"), entries);

    assert_eq!(panel.path(), Path::new("/home/user"));
    assert_eq!(panel.rows().len(), 2);
    assert_eq!(panel.rows()[0], Row::Parent);
}

#[test]
fn from_listing_at_root_has_no_parent_row() {
    let panel = PanelView::from_listing(PathBuf::from("/"), Vec::new());

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
    let mut panel = PanelView::from_listing(PathBuf::from("/home/user"), entries);

    assert_eq!(panel.target_path_for_open(), Some(PathBuf::from("/home")));

    panel.cursor = 1;
    assert_eq!(panel.target_path_for_open(), Some(PathBuf::from("/home/user/child")));
}

fn file(dir: &str, name: &str, size: u64) -> Entry {
    Entry { name: name.to_string(), path: PathBuf::from(dir).join(name), is_dir: false, size, permissions: None }
}

fn folder(dir: &str, name: &str) -> Entry {
    Entry { name: name.to_string(), path: PathBuf::from(dir).join(name), is_dir: true, size: 0, permissions: None }
}

fn names(panel: &PanelView) -> Vec<String> {
    panel
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Entry(entry) => Some(entry.name.clone()),
            Row::Parent => None,
        })
        .collect()
}

#[test]
fn new_panel_lists_the_given_directory() {
    let panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "file.txt", 7)]);

    assert_eq!(panel.rows().len(), 2);
    assert_eq!(panel.rows()[0], Row::Parent);
}

#[test]
fn move_cursor_clamps_within_bounds() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "file.txt", 7)]);

    panel.move_cursor(-5);
    assert_eq!(panel.cursor, 0);

    panel.move_cursor(5);
    assert_eq!(panel.cursor, panel.rows().len() - 1);
}

#[test]
fn navigate_to_changes_the_path_and_refreshes() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![folder("/d", "child")]);

    panel.replace_listing(PathBuf::from("/d/child"), vec![file("/d/child", "inner.txt", 1)]);

    assert_eq!(panel.path(), Path::new("/d/child"));
    assert_eq!(names(&panel), ["inner.txt"]);
}

#[test]
fn open_selected_navigates_into_a_directory() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![folder("/d", "child")]);
    panel.cursor = panel.rows().len() - 1;

    assert_eq!(panel.target_path_for_open(), Some(PathBuf::from("/d/child")));
}

#[test]
fn open_selected_on_parent_row_navigates_up() {
    let panel = PanelView::from_listing(PathBuf::from("/d/child"), Vec::new());
    assert_eq!(panel.rows()[0], Row::Parent);

    assert_eq!(panel.target_path_for_open(), Some(PathBuf::from("/d")));
}

#[test]
fn opening_a_file_row_goes_nowhere() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "a.txt", 1)]);
    panel.cursor = 1;

    assert_eq!(panel.target_path_for_open(), None);
}

#[test]
fn toggle_selection_adds_and_removes_the_current_entry() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "file.txt", 7)]);
    panel.cursor = panel.rows().len() - 1;

    panel.toggle_selection();
    assert_eq!(panel.selected.len(), 1);

    panel.toggle_selection();
    assert_eq!(panel.selected.len(), 0);
}

#[test]
fn replacing_the_listing_clears_the_selection() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "file.txt", 7)]);
    panel.cursor = 1;
    panel.toggle_selection();

    panel.replace_listing(PathBuf::from("/d"), vec![file("/d", "file.txt", 7)]);

    assert!(panel.selected.is_empty());
}

#[test]
fn target_entries_returns_the_cursor_entry_without_a_selection() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "only.txt", 7)]);
    panel.cursor = panel.rows().len() - 1;

    let entries = panel.target_entries();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "only.txt");
}

#[test]
fn target_entries_returns_all_selected_entries() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "a.txt", 7), file("/d", "b.txt", 7)]);
    panel.cursor = 1;
    panel.toggle_selection();
    panel.cursor = 2;
    panel.toggle_selection();

    let mut names: Vec<String> = panel.target_entries().into_iter().map(|entry| entry.name).collect();
    names.sort();

    assert_eq!(names, vec!["a.txt".to_string(), "b.txt".to_string()]);
}

#[test]
fn delete_targets_removes_selected_entries() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "a.txt", 7), file("/d", "b.txt", 7)]);
    panel.cursor = 1;
    panel.toggle_selection();
    panel.cursor = 2;
    panel.toggle_selection();

    let mut targets = panel.targets();
    targets.sort();

    assert_eq!(targets, vec![PathBuf::from("/d/a.txt"), PathBuf::from("/d/b.txt")]);
}

#[test]
fn delete_targets_falls_back_to_cursor_entry_without_selection() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "only.txt", 7)]);
    panel.cursor = panel.rows().len() - 1;

    assert_eq!(panel.targets(), vec![PathBuf::from("/d/only.txt")]);
}

#[test]
fn render_panel_draws_the_given_title() {
    let panel = PanelView::from_listing(PathBuf::from("/d"), Vec::new());
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
    let panel =
        PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", ".hidden", 1), file("/d", "visible.txt", 1)]);

    assert_eq!(names(&panel), vec!["visible.txt".to_string()]);
}

#[test]
fn toggle_hidden_reveals_dotfiles_without_touching_the_filesystem() {
    let mut panel = PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", ".hidden", 1)]);

    panel.toggle_hidden();
    assert_eq!(names(&panel), vec![".hidden".to_string()]);

    panel.toggle_hidden();
    assert_eq!(panel.rows().len(), 1);
}

#[test]
fn cycle_sort_reorders_rows_by_size_when_advanced_twice() {
    let mut panel =
        PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "big.txt", 100), file("/d", "small.txt", 1)]);

    panel.cycle_sort();
    panel.cycle_sort();

    assert_eq!(names(&panel), vec!["small.txt".to_string(), "big.txt".to_string()]);
}

#[test]
fn set_sort_spec_and_set_show_hidden_apply_immediately() {
    let spec = SortSpec { key: SortKey::Size, order: SortOrder::Descending };
    let mut panel = PanelView::new(PathBuf::from("/d"), spec, true);

    panel.replace_listing(PathBuf::from("/d"), vec![file("/d", ".hidden", 1)]);

    assert_eq!(panel.rows().len(), 2);
    assert_eq!(panel.sort_spec(), spec);
}

#[test]
fn a_shrinking_listing_keeps_the_cursor_on_a_row() {
    let mut panel =
        PanelView::from_listing(PathBuf::from("/d"), vec![file("/d", "a", 1), file("/d", "b", 1), file("/d", "c", 1)]);
    panel.cursor = 3;

    panel.replace_listing(PathBuf::from("/d"), vec![file("/d", "a", 1)]);

    assert_eq!(panel.cursor, 1);
}
