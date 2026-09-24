use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};
pub use termconnect_core::listing::Row;
use termconnect_core::{
    Entry,
    listing::{Listing, SortKey, SortOrder, SortSpec},
};

use crate::widgets::file_list;

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

pub struct PanelView {
    listing: Listing,
    pub cursor: usize,
    pub selected: HashSet<PathBuf>,
}

impl PanelView {
    pub fn new(path: PathBuf, sort_spec: SortSpec, show_hidden: bool) -> Self {
        Self { listing: Listing::new(path, Vec::new(), sort_spec, show_hidden), cursor: 0, selected: HashSet::new() }
    }

    pub fn from_listing(path: PathBuf, entries: Vec<Entry>) -> Self {
        let mut panel = Self::new(PathBuf::new(), SortSpec::default(), false);
        panel.replace_listing(path, entries);
        panel
    }

    pub fn path(&self) -> &Path {
        self.listing.path()
    }

    pub fn rows(&self) -> &[Row] {
        self.listing.rows()
    }

    pub fn replace_listing(&mut self, path: PathBuf, entries: Vec<Entry>) {
        self.listing.replace(path, entries);
        self.selected.clear();
        self.clamp_cursor();
    }

    pub fn toggle_hidden(&mut self) {
        self.listing.toggle_hidden();
        self.clamp_cursor();
    }

    pub fn cycle_sort(&mut self) {
        self.listing.cycle_sort();
        self.clamp_cursor();
    }

    pub fn sort_spec(&self) -> SortSpec {
        self.listing.sort_spec()
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.rows().is_empty() {
            return;
        }

        let max = self.rows().len() as isize - 1;
        let next = (self.cursor as isize + delta).clamp(0, max);
        self.cursor = next as usize;
    }

    pub fn target_path_for_open(&self) -> Option<PathBuf> {
        match self.rows().get(self.cursor) {
            Some(Row::Parent) => self.path().parent().map(Path::to_path_buf),
            Some(Row::Entry(entry)) if entry.is_dir => Some(entry.path.clone()),
            _ => None,
        }
    }

    pub fn toggle_selection(&mut self) {
        if let Some(Row::Entry(entry)) = self.rows().get(self.cursor) {
            let path = entry.path.clone();
            if !self.selected.remove(&path) {
                self.selected.insert(path);
            }
        }
    }

    pub fn current_entry_name(&self) -> Option<&str> {
        match self.rows().get(self.cursor) {
            Some(Row::Entry(entry)) => Some(entry.name.as_str()),
            _ => None,
        }
    }

    pub fn targets(&self) -> Vec<PathBuf> {
        if !self.selected.is_empty() {
            return self.selected.iter().cloned().collect();
        }

        match self.rows().get(self.cursor) {
            Some(Row::Entry(entry)) => vec![entry.path.clone()],
            _ => Vec::new(),
        }
    }

    pub fn target_entries(&self) -> Vec<Entry> {
        if !self.selected.is_empty() {
            return self
                .rows()
                .iter()
                .filter_map(|row| match row {
                    Row::Entry(entry) if self.selected.contains(&entry.path) => Some(entry.clone()),
                    _ => None,
                })
                .collect();
        }

        match self.rows().get(self.cursor) {
            Some(Row::Entry(entry)) => vec![entry.clone()],
            _ => Vec::new(),
        }
    }

    fn clamp_cursor(&mut self) {
        let rows = self.rows().len();
        if rows == 0 {
            self.cursor = 0;
        } else if self.cursor >= rows {
            self.cursor = rows - 1;
        }
    }
}

pub fn render_panel(frame: &mut Frame, area: Rect, title: &str, is_active: bool, panel: &PanelView) {
    let border_style = if is_active { Style::default().fg(Color::Yellow) } else { Style::default() };

    let block = Block::default()
        .title(format!("{title} {} [{}]", panel.path().display(), sort_indicator(panel.sort_spec())))
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
    let border_style = if is_active { Style::default().fg(Color::Yellow) } else { Style::default() };

    let block = Block::default().title(title).borders(Borders::ALL).border_style(border_style);

    frame.render_widget(block, area);
}

#[cfg(test)]
mod tests;
