pub mod sort;

use std::path::{Path, PathBuf};

use porthmos_vfs::{Entry, glob_match};
pub use sort::{SortKey, SortOrder, SortSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Parent,
    Entry(Entry),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    path: PathBuf,
    entries: Vec<Entry>,
    rows: Vec<Row>,
    sort_spec: SortSpec,
    show_hidden: bool,
    filter: Option<String>,
    shown: usize,
}

fn name_matches(filter: &str, lowered: &str, name: &str) -> bool {
    if filter.contains(['*', '?']) { glob_match(filter, name) } else { name.to_lowercase().contains(lowered) }
}

impl Listing {
    pub fn new(path: PathBuf, entries: Vec<Entry>, sort_spec: SortSpec, show_hidden: bool) -> Self {
        let mut listing = Self { path, entries, rows: Vec::new(), sort_spec, show_hidden, filter: None, shown: 0 };
        listing.recompute_rows();
        listing
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn replace(&mut self, path: PathBuf, entries: Vec<Entry>) {
        if path != self.path {
            self.filter = None;
        }
        self.path = path;
        self.entries = entries;
        self.recompute_rows();
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

    pub fn set_filter(&mut self, filter: Option<&str>) {
        self.filter = filter.filter(|text| !text.is_empty()).map(str::to_string);
        self.recompute_rows();
    }

    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    pub fn match_count(&self) -> (usize, usize) {
        let matched = self.rows.iter().filter(|row| matches!(row, Row::Entry(_))).count();
        (matched, self.shown)
    }

    pub fn sort_spec(&self) -> SortSpec {
        self.sort_spec
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    fn recompute_rows(&mut self) {
        let shown: Vec<&Entry> =
            self.entries.iter().filter(|entry| self.show_hidden || !entry.name.starts_with('.')).collect();
        self.shown = shown.len();
        let lowered = self.filter.as_deref().map(str::to_lowercase).unwrap_or_default();
        let mut visible: Vec<Entry> = shown
            .into_iter()
            .filter(|entry| self.filter.as_deref().is_none_or(|filter| name_matches(filter, &lowered, &entry.name)))
            .cloned()
            .collect();
        sort::sort_entries(&mut visible, self.sort_spec);

        let mut rows = Vec::with_capacity(visible.len() + 1);
        if self.path.parent().is_some() {
            rows.push(Row::Parent);
        }
        rows.extend(visible.into_iter().map(Row::Entry));
        self.rows = rows;
    }
}

#[cfg(test)]
mod tests;
