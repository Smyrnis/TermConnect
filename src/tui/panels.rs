use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders};

use crate::filesystem::{Entry, local};
use crate::tui::file_list;
use crate::tui::sort::{self, SortKey, SortOrder, SortSpec};

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
    all_entries: Vec<Entry>,
    rows: Vec<Row>,
    pub cursor: usize,
    pub selected: HashSet<PathBuf>,
    sort_spec: SortSpec,
    show_hidden: bool,
}

impl PanelState {
    pub fn new(path: PathBuf) -> Result<Self> {
        let mut panel = Self {
            path,
            all_entries: Vec::new(),
            rows: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            sort_spec: SortSpec::default(),
            show_hidden: false,
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
            all_entries: Vec::new(),
            rows: Vec::new(),
            cursor: 0,
            selected: HashSet::new(),
            sort_spec: SortSpec::default(),
            show_hidden: false,
        };
        panel.replace_listing(path, entries);
        panel
    }

    /// Replaces the current listing with an already-fetched one, without
    /// touching the filesystem — the async counterpart to `refresh`.
    pub fn replace_listing(&mut self, path: PathBuf, entries: Vec<Entry>) {
        self.path = path;
        self.all_entries = entries;
        self.selected.clear();
        self.recompute_rows();
    }

    /// Rebuilds `rows` from `all_entries` by applying `show_hidden` and
    /// `sort_spec` — pure state, touches neither the filesystem nor the
    /// network, so toggling either is instant.
    fn recompute_rows(&mut self) {
        let mut visible: Vec<Entry> = self
            .all_entries
            .iter()
            .filter(|entry| self.show_hidden || !entry.name.starts_with('.'))
            .cloned()
            .collect();
        sort::sort_entries(&mut visible, self.sort_spec);

        let mut rows = Vec::with_capacity(visible.len() + 1);
        if self.path.parent().is_some() {
            rows.push(Row::Parent);
        }
        rows.extend(visible.into_iter().map(Row::Entry));

        self.rows = rows;
        self.clamp_cursor();
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.recompute_rows();
    }

    pub fn cycle_sort(&mut self) {
        self.sort_spec = self.sort_spec.cycled();
        self.recompute_rows();
    }

    pub fn set_sort_spec(&mut self, spec: SortSpec) {
        self.sort_spec = spec;
        self.recompute_rows();
    }

    pub fn set_show_hidden(&mut self, show_hidden: bool) {
        self.show_hidden = show_hidden;
        self.recompute_rows();
    }

    pub fn sort_spec(&self) -> SortSpec {
        self.sort_spec
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
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

    /// Navigates directly to `path` (used by bookmarks). Sets the cursor
    /// to the top and refreshes, same as opening a directory normally.
    pub fn navigate_to(&mut self, path: PathBuf) -> Result<()> {
        self.path = path;
        self.cursor = 0;
        self.refresh()
    }

    /// Enters the directory at the cursor (or the parent, for `..`).
    /// A no-op if the cursor is on a file. Local-panel only — the remote
    /// panel navigates by fetching a new listing asynchronously instead
    /// (see `target_path_for_open`).
    pub fn open_selected(&mut self) -> Result<()> {
        if let Some(target) = self.target_path_for_open() {
            self.navigate_to(target)?;
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

    /// Like `targets`, but returns the full entries (name, size, is_dir)
    /// rather than just their paths — what a transfer needs to know.
    pub fn target_entries(&self) -> Vec<Entry> {
        if !self.selected.is_empty() {
            return self
                .rows
                .iter()
                .filter_map(|row| match row {
                    Row::Entry(entry) if self.selected.contains(&entry.path) => Some(entry.clone()),
                    _ => None,
                })
                .collect();
        }

        match self.rows.get(self.cursor) {
            Some(Row::Entry(entry)) => vec![entry.clone()],
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
        .title(format!(
            "{title} {} [{}]",
            panel.path().display(),
            sort_indicator(panel.sort_spec())
        ))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);
    file_list::render_file_list(frame, inner, panel, is_active);
}

fn sort_indicator(spec: SortSpec) -> String {
    let key = match spec.key {
        SortKey::Name => "Name",
        SortKey::Size => "Size",
    };
    let arrow = match spec.order {
        SortOrder::Ascending => '\u{25B2}',
        SortOrder::Descending => '\u{25BC}',
    };
    format!("{key} {arrow}")
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
#[path = "../../tests/tui/panels_test.rs"]
mod tests;
