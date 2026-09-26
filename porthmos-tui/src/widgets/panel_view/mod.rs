use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub use porthmos_core::listing::Row;
use porthmos_core::{
    Entry,
    listing::{Listing, SortKey, SortOrder, SortSpec},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders},
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Anchor {
    Parent,
    Entry(PathBuf),
}

pub struct PanelView {
    listing: Listing,
    pub cursor: usize,
    pub selected: HashSet<PathBuf>,
    editing_filter: bool,
    anchor: Option<(Anchor, usize)>,
}

impl PanelView {
    pub fn new(path: PathBuf, sort_spec: SortSpec, show_hidden: bool) -> Self {
        Self {
            listing: Listing::new(path, Vec::new(), sort_spec, show_hidden),
            cursor: 0,
            selected: HashSet::new(),
            editing_filter: false,
            anchor: None,
        }
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
        if path != self.path() {
            self.editing_filter = false;
        }
        self.listing.replace(path, entries);
        self.anchor = None;
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

    pub fn filter(&self) -> Option<&str> {
        self.listing.filter()
    }

    pub fn editing_filter(&self) -> bool {
        self.editing_filter
    }

    pub fn start_filter(&mut self) {
        self.editing_filter = true;
    }

    pub fn finish_filter(&mut self) {
        self.editing_filter = false;
    }

    pub fn type_filter(&mut self, character: char) {
        let mut text = self.filter().unwrap_or_default().to_string();
        text.push(character);
        self.apply_filter(Some(text));
    }

    pub fn erase_filter(&mut self) {
        let mut text = self.filter().unwrap_or_default().to_string();
        text.pop();
        self.apply_filter(Some(text));
    }

    pub fn clear_filter(&mut self) {
        self.editing_filter = false;
        self.apply_filter(None);
        self.anchor = None;
    }

    fn apply_filter(&mut self, text: Option<String>) {
        let anchor = match &self.anchor {
            Some((anchor, placed)) if *placed == self.cursor => Some(anchor.clone()),
            _ => self.anchor_at_cursor(),
        };
        self.listing.set_filter(text.as_deref());
        let kept = anchor
            .as_ref()
            .filter(|anchor| self.filter().is_none() || **anchor != Anchor::Parent)
            .and_then(|anchor| self.position_of(anchor));
        let first_match = || self.rows().iter().position(|row| matches!(row, Row::Entry(_)));
        self.cursor = kept.or_else(first_match).unwrap_or(0);
        self.anchor = anchor.map(|anchor| (anchor, self.cursor));
    }

    fn anchor_at_cursor(&self) -> Option<Anchor> {
        match self.rows().get(self.cursor)? {
            Row::Parent => Some(Anchor::Parent),
            Row::Entry(entry) => Some(Anchor::Entry(entry.path.clone())),
        }
    }

    fn position_of(&self, anchor: &Anchor) -> Option<usize> {
        self.rows().iter().position(|row| match (row, anchor) {
            (Row::Parent, Anchor::Parent) => true,
            (Row::Entry(entry), Anchor::Entry(path)) => entry.path == *path,
            _ => false,
        })
    }

    fn filter_status(&self, width: usize) -> Option<String> {
        let (matched, total) = self.listing.match_count();
        let (prefix, text, suffix) = match (self.editing_filter, self.filter()) {
            (true, text) => ("/", text.unwrap_or_default(), format!("\u{2588} ({matched} of {total})")),
            (false, Some(text)) => ("filter: ", text, format!(" ({matched} of {total})")),
            (false, None) => return None,
        };
        let room = width.saturating_sub(prefix.chars().count() + suffix.chars().count());
        let length = text.chars().count();
        let shown = if length <= room {
            text.to_string()
        } else {
            let tail: String = text.chars().skip(length - room.saturating_sub(1)).collect();
            format!("\u{2026}{tail}")
        };
        Some(format!("{prefix}{shown}{suffix}"))
    }

    fn visible_selected(&self) -> Vec<&Entry> {
        self.rows()
            .iter()
            .filter_map(|row| match row {
                Row::Entry(entry) if self.selected.contains(&entry.path) => Some(entry),
                _ => None,
            })
            .collect()
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
        let selected = self.visible_selected();
        if !selected.is_empty() {
            return selected.into_iter().map(|entry| entry.path.clone()).collect();
        }

        match self.rows().get(self.cursor) {
            Some(Row::Entry(entry)) => vec![entry.path.clone()],
            _ => Vec::new(),
        }
    }

    pub fn target_entries(&self) -> Vec<Entry> {
        let selected = self.visible_selected();
        if !selected.is_empty() {
            return selected.into_iter().cloned().collect();
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

    let mut block = Block::default()
        .title(format!("{title} {} [{}]", panel.path().display(), sort_indicator(panel.sort_spec())))
        .borders(Borders::ALL)
        .border_style(border_style);
    if let Some(status) = panel.filter_status(area.width.saturating_sub(2) as usize) {
        block = block.title_bottom(Line::from(status));
    }

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
