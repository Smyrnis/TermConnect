pub mod sort;

use std::path::{Path, PathBuf};

pub use sort::{SortKey, SortOrder, SortSpec};
use termconnect_vfs::Entry;

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
}

impl Listing {
    pub fn new(path: PathBuf, entries: Vec<Entry>, sort_spec: SortSpec, show_hidden: bool) -> Self {
        let mut listing = Self { path, entries, rows: Vec::new(), sort_spec, show_hidden };
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

    pub fn sort_spec(&self) -> SortSpec {
        self.sort_spec
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    fn recompute_rows(&mut self) {
        let mut visible: Vec<Entry> =
            self.entries.iter().filter(|entry| self.show_hidden || !entry.name.starts_with('.')).cloned().collect();
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
