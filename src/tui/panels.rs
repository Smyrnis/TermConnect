use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders};

use crate::filesystem::{Entry, local};
use crate::tui::widgets::file_list;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    Local,
    Remote,
}

impl ActivePanel {
    pub fn toggle(&mut self) {
        *self = match self {
            ActivePanel::Local => ActivePanel::Remote,
            ActivePanel::Remote => ActivePanel::Local,
        };
    }
}

/// One line in a panel's listing: either the synthetic ".." entry used to
/// navigate to the parent directory, or a real filesystem entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Parent,
    Entry(Entry),
}

pub struct PanelState {
    path: PathBuf,
    rows: Vec<Row>,
    pub cursor: usize,
    pub selected: HashSet<PathBuf>,
}

impl PanelState {
    pub fn new(path: PathBuf) -> Result<Self> {
        let mut panel = Self {
            path,
            rows: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
        };
        panel.refresh()?;
        Ok(panel)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Builds a panel directly from an already-fetched listing, bypassing
    /// `local::list` — used for the remote panel, whose listings come back
    /// from an async SFTP call rather than a synchronous filesystem read.
    pub fn from_listing(path: PathBuf, entries: Vec<Entry>) -> Self {
        let mut panel = Self {
            path: PathBuf::new(),
            rows: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
        };
        panel.replace_listing(path, entries);
        panel
    }

    /// Replaces the current listing with an already-fetched one, without
    /// touching the filesystem — the async counterpart to `refresh`.
    pub fn replace_listing(&mut self, path: PathBuf, entries: Vec<Entry>) {
        let mut rows: Vec<Row> = Vec::with_capacity(entries.len() + 1);

        if path.parent().is_some() {
            rows.push(Row::Parent);
        }

        rows.extend(entries.into_iter().map(Row::Entry));

        self.path = path;
        self.rows = rows;
        self.selected.clear();
        self.clamp_cursor();
    }

    pub fn refresh(&mut self) -> Result<()> {
        let entries = local::list(&self.path)?;
        self.replace_listing(self.path.clone(), entries);
        Ok(())
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }

        let max = self.rows.len() as isize - 1;
        let next = (self.cursor as isize + delta).clamp(0, max);
        self.cursor = next as usize;
    }

    /// Where `Open` would navigate to: the parent directory for `..`, a
    /// subdirectory's path for a directory entry, or `None` for a file (or
    /// an empty listing). Pure — does not touch the filesystem or mutate
    /// state, so both the synchronous local path and the async remote path
    /// can use it to decide where to fetch next.
    pub fn target_path_for_open(&self) -> Option<PathBuf> {
        match self.rows.get(self.cursor) {
            Some(Row::Parent) => self.path.parent().map(Path::to_path_buf),
            Some(Row::Entry(entry)) if entry.is_dir => Some(entry.path.clone()),
            _ => None,
        }
    }

    /// Enters the directory at the cursor (or the parent, for `..`).
    /// A no-op if the cursor is on a file. Local-panel only — the remote
    /// panel navigates by fetching a new listing asynchronously instead
    /// (see `target_path_for_open`).
    pub fn open_selected(&mut self) -> Result<()> {
        if let Some(target) = self.target_path_for_open() {
            self.path = target;
            self.cursor = 0;
            self.refresh()?;
        }

        Ok(())
    }

    /// Toggles selection of the entry at the cursor. Selecting `..` is a no-op.
    pub fn toggle_selection(&mut self) {
        if let Some(Row::Entry(entry)) = self.rows.get(self.cursor) {
            let path = entry.path.clone();
            if !self.selected.remove(&path) {
                self.selected.insert(path);
            }
        }
    }

    /// The name of the entry at the cursor, for pre-filling a rename dialog.
    /// `None` for `..` or an empty listing.
    pub fn current_entry_name(&self) -> Option<&str> {
        match self.rows.get(self.cursor) {
            Some(Row::Entry(entry)) => Some(entry.name.as_str()),
            _ => None,
        }
    }

    /// The paths a delete/rename should act on: the selection if non-empty,
    /// otherwise just the entry under the cursor.
    pub fn targets(&self) -> Vec<PathBuf> {
        if !self.selected.is_empty() {
            return self.selected.iter().cloned().collect();
        }

        match self.rows.get(self.cursor) {
            Some(Row::Entry(entry)) => vec![entry.path.clone()],
            _ => Vec::new(),
        }
    }

    pub fn create_directory(&mut self, name: &str) -> Result<()> {
        local::create_directory(&self.path.join(name))?;
        self.refresh()
    }

    pub fn rename_current(&mut self, new_name: &str) -> Result<()> {
        if let Some(Row::Entry(entry)) = self.rows.get(self.cursor).cloned() {
            let destination = self.path.join(new_name);
            local::rename(&entry.path, &destination)?;
        }
        self.refresh()
    }

    pub fn delete_targets(&mut self) -> Result<()> {
        for path in self.targets() {
            local::delete(&path)?;
        }
        self.refresh()
    }

    fn clamp_cursor(&mut self) {
        if self.rows.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len() - 1;
        }
    }
}

pub fn render_panel(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    is_active: bool,
    panel: &PanelState,
) {
    let border_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(format!("{title} {}", panel.path().display()))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);
    file_list::render_file_list(frame, inner, panel, is_active);
}

pub fn render_placeholder(frame: &mut Frame, area: Rect, title: &str, is_active: bool) {
    let border_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    frame.render_widget(block, area);
}

#[cfg(test)]
mod tests {
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
        }];

        let panel = PanelState::from_listing(PathBuf::from("/home/user"), entries);

        assert_eq!(panel.path(), Path::new("/home/user"));
        assert_eq!(panel.rows().len(), 2); // Parent + the one entry
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
        }];
        let mut panel = PanelState::from_listing(PathBuf::from("/home/user"), entries);

        assert_eq!(panel.target_path_for_open(), Some(PathBuf::from("/home")));

        panel.cursor = 1;
        assert_eq!(
            panel.target_path_for_open(),
            Some(PathBuf::from("/home/user/child"))
        );
    }

    #[test]
    fn new_panel_lists_the_given_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("file.txt"), b"content").unwrap();

        let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

        // Parent row + the one file.
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
    fn open_selected_navigates_into_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("child")).unwrap();
        let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();
        panel.cursor = panel.rows().len() - 1; // the child directory row

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
    fn create_directory_adds_a_new_row_after_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let mut panel = PanelState::new(dir.path().to_path_buf()).unwrap();

        panel.create_directory("new_dir").unwrap();

        assert!(dir.path().join("new_dir").is_dir());
        assert!(
            panel
                .rows()
                .iter()
                .any(|row| matches!(row, Row::Entry(entry) if entry.name == "new_dir"))
        );
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

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("LOCAL"));
    }
}
