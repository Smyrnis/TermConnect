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

fn filter_panel() -> PanelView {
    PanelView::from_listing(
        PathBuf::from("/d"),
        vec![
            folder("/d", "porthmos"),
            file("/d", "Report.pdf", 1),
            file("/d", "notes.txt", 2),
            file("/d", "app.log", 3),
        ],
    )
}

fn type_text(panel: &mut PanelView, text: &str) {
    for character in text.chars() {
        panel.type_filter(character);
    }
}

#[test]
fn typing_in_the_filter_line_narrows_the_panel() {
    let mut panel = filter_panel();

    panel.start_filter();
    type_text(&mut panel, "port");

    assert!(panel.editing_filter());
    assert_eq!(panel.filter(), Some("port"));
    assert_eq!(names(&panel), vec!["porthmos".to_string(), "Report.pdf".to_string()]);
}

#[test]
fn erasing_and_finishing_keep_the_filter_and_clearing_drops_it() {
    let mut panel = filter_panel();
    panel.start_filter();
    type_text(&mut panel, "pdfx");

    panel.erase_filter();
    panel.finish_filter();
    assert!(!panel.editing_filter());
    assert_eq!(names(&panel), vec!["Report.pdf".to_string()]);

    panel.clear_filter();
    assert_eq!(panel.filter(), None);
    assert_eq!(names(&panel).len(), 4);
}

#[test]
fn the_cursor_stays_on_its_file_while_it_matches_and_jumps_to_the_first_match_otherwise() {
    let mut panel = filter_panel();
    let report =
        panel.rows().iter().position(|row| matches!(row, Row::Entry(entry) if entry.name == "Report.pdf")).unwrap();
    panel.cursor = report;

    panel.start_filter();
    type_text(&mut panel, "r");
    assert_eq!(panel.current_entry_name(), Some("Report.pdf"));

    type_text(&mut panel, "th");
    assert_eq!(panel.current_entry_name(), Some("porthmos"));
}

#[test]
fn only_visible_selected_files_are_targets() {
    let mut panel = filter_panel();
    for name in ["Report.pdf", "notes.txt"] {
        panel.cursor =
            panel.rows().iter().position(|row| matches!(row, Row::Entry(entry) if entry.name == name)).unwrap();
        panel.toggle_selection();
    }

    panel.start_filter();
    type_text(&mut panel, "pdf");

    assert_eq!(panel.targets(), vec![PathBuf::from("/d/Report.pdf")]);
    assert_eq!(panel.target_entries().iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), vec!["Report.pdf"]);
    panel.clear_filter();
    assert_eq!(panel.targets().len(), 2);
}

#[test]
fn opening_another_folder_closes_the_filter_line() {
    let mut panel = filter_panel();
    panel.start_filter();
    type_text(&mut panel, "port");

    panel.replace_listing(PathBuf::from("/d/porthmos"), vec![file("/d/porthmos", "x", 1)]);

    assert!(!panel.editing_filter());
    assert_eq!(panel.filter(), None);
}

fn rendered(panel: &PanelView) -> String {
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|frame| render_panel(frame, Rect::new(0, 0, 40, 8), "LOCAL", true, panel)).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn the_bottom_border_shows_the_filter_line_or_the_active_filter() {
    let mut panel = filter_panel();
    panel.start_filter();
    type_text(&mut panel, "port");

    assert!(rendered(&panel).contains("/port\u{2588} (2 of 4)"), "{}", rendered(&panel));
    panel.finish_filter();
    assert!(rendered(&panel).contains("filter: port (2 of 4)"), "{}", rendered(&panel));
}

#[test]
fn a_refresh_keeps_the_filter_and_the_open_line() {
    let mut panel = filter_panel();
    panel.start_filter();
    type_text(&mut panel, "port");

    panel.replace_listing(PathBuf::from("/d"), vec![folder("/d", "porthmos"), file("/d", "passport.txt", 1)]);

    assert!(panel.editing_filter());
    assert_eq!(panel.filter(), Some("port"));
    assert_eq!(names(&panel), vec!["porthmos".to_string(), "passport.txt".to_string()]);
}

#[test]
fn showing_hidden_files_counts_them_and_filters_them() {
    let mut panel = PanelView::from_listing(
        PathBuf::from("/d"),
        vec![file("/d", ".profile", 1), file("/d", "profile.txt", 2), file("/d", "notes", 3)],
    );
    panel.start_filter();
    type_text(&mut panel, "prof");
    assert!(rendered(&panel).contains("(1 of 2)"), "{}", rendered(&panel));

    panel.toggle_hidden();

    assert_eq!(names(&panel), vec![".profile".to_string(), "profile.txt".to_string()]);
    assert!(rendered(&panel).contains("(2 of 3)"), "{}", rendered(&panel));
}

#[test]
fn clearing_the_filter_returns_the_cursor_to_the_parent_row() {
    let mut panel = filter_panel();
    panel.cursor = 0;

    panel.start_filter();
    type_text(&mut panel, "o");
    assert_eq!(panel.current_entry_name(), Some("porthmos"));
    panel.clear_filter();

    assert_eq!(panel.cursor, 0);
}

#[test]
fn the_cursor_returns_to_its_file_after_nothing_matched() {
    let mut panel = filter_panel();
    panel.cursor =
        panel.rows().iter().position(|row| matches!(row, Row::Entry(entry) if entry.name == "notes.txt")).unwrap();

    panel.start_filter();
    type_text(&mut panel, "tz");
    panel.erase_filter();

    assert_eq!(panel.current_entry_name(), Some("notes.txt"));
}

fn rendered_at(panel: &PanelView, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 8)).unwrap();
    terminal.draw(|frame| render_panel(frame, Rect::new(0, 0, width, 8), "LOCAL", true, panel)).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn a_narrow_panel_keeps_the_end_of_the_filter_the_caret_and_the_count() {
    let mut panel = filter_panel();
    panel.start_filter();
    type_text(&mut panel, "a_really_long_filter_text");

    let screen = rendered_at(&panel, 24);

    assert!(screen.contains("/\u{2026}"), "{screen}");
    assert!(screen.contains("_text\u{2588} (0 of 4)"), "{screen}");
}
