use std::collections::HashSet;

use porthmos_core::{
    connections_tree::{self, TreeRow},
    profiles::{ConnectionEntry, ConnectionSource},
};

use crate::widgets::filter_line::{self, FilterLine};

#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Group(String),
    Connection(String, ConnectionSource),
}

pub enum Selection<'a> {
    Group(&'a str),
    Connection(&'a ConnectionEntry),
}

#[derive(Default)]
pub struct ConnectionsView {
    entries: Vec<ConnectionEntry>,
    expanded: HashSet<String>,
    filter: Option<String>,
    editing_filter: bool,
    rows: Vec<TreeRow>,
    pub cursor: usize,
}

impl ConnectionsView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[ConnectionEntry] {
        &self.entries
    }

    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    pub fn replace(&mut self, entries: Vec<ConnectionEntry>) {
        let key = self.key_at_cursor();
        self.entries = entries;
        self.rebuild(key);
    }

    pub fn selected(&self) -> Option<Selection<'_>> {
        match self.rows.get(self.cursor)? {
            TreeRow::Group { path, .. } => Some(Selection::Group(path)),
            TreeRow::Connection { index, .. } => Some(Selection::Connection(&self.entries[*index])),
        }
    }

    pub fn selected_entry(&self) -> Option<&ConnectionEntry> {
        match self.selected()? {
            Selection::Connection(entry) => Some(entry),
            Selection::Group(_) => None,
        }
    }

    fn filtering(&self) -> bool {
        connections_tree::active_filter(self.filter.as_deref()).is_some()
    }

    pub fn toggle(&mut self) {
        if self.filtering() {
            return;
        }
        if let Some(Selection::Group(path)) = self.selected() {
            let path = path.to_string();
            if !self.expanded.remove(&path) {
                self.expanded.insert(path.clone());
            }
            self.rebuild(Some(RowKey::Group(path)));
        }
    }

    pub fn expand(&mut self) {
        if self.filtering() {
            return;
        }
        if let Some(Selection::Group(path)) = self.selected() {
            let path = path.to_string();
            self.expanded.insert(path.clone());
            self.rebuild(Some(RowKey::Group(path)));
        }
    }

    pub fn collapse_or_parent(&mut self) {
        let parent = match self.rows.get(self.cursor) {
            Some(TreeRow::Group { path, expanded: true, .. }) if !self.filtering() => {
                let path = path.clone();
                self.expanded.remove(&path);
                return self.rebuild(Some(RowKey::Group(path)));
            }
            Some(TreeRow::Group { path, .. }) => path.rsplit_once('/').map(|(parent, _)| parent.to_string()),
            Some(TreeRow::Connection { index, .. }) => self.entries[*index].group.clone(),
            None => None,
        };
        if let Some(parent) = parent {
            self.rebuild(Some(RowKey::Group(parent)));
        }
    }

    pub fn reveal(&mut self, name: &str) {
        let Some(entry) = self.entries.iter().find(|entry| entry.name == name && !entry.source.is_orphan_labels())
        else {
            return;
        };
        let (group, source) = (entry.group.clone(), entry.source);
        if let Some(group) = group {
            let mut path = String::new();
            for segment in group.split('/') {
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(segment);
                self.expanded.insert(path.clone());
            }
        }
        self.rebuild(Some(RowKey::Connection(name.to_string(), source)));
    }

    pub fn filter_status(&self, width: usize) -> Option<String> {
        let matched = connections_tree::match_count(&self.entries, self.filter.as_deref());
        filter_line::status(self.editing_filter, self.filter.as_deref(), matched, self.entries.len(), width)
    }

    fn key_at_cursor(&self) -> Option<RowKey> {
        match self.rows.get(self.cursor)? {
            TreeRow::Group { path, .. } => Some(RowKey::Group(path.clone())),
            TreeRow::Connection { index, .. } => {
                let entry = &self.entries[*index];
                Some(RowKey::Connection(entry.name.clone(), entry.source))
            }
        }
    }

    fn position_of(&self, key: &RowKey) -> Option<usize> {
        self.rows.iter().position(|row| match (row, key) {
            (TreeRow::Group { path, .. }, RowKey::Group(wanted)) => path == wanted,
            (TreeRow::Connection { index, .. }, RowKey::Connection(name, source)) => {
                self.entries[*index].name == *name && self.entries[*index].source == *source
            }
            _ => false,
        })
    }

    fn rebuild(&mut self, keep: Option<RowKey>) {
        self.rows = connections_tree::rows(&self.entries, &self.expanded, self.filter.as_deref());
        match keep.and_then(|key| self.position_of(&key)) {
            Some(position) => self.cursor = position,
            None => self.cursor = self.cursor.min(self.rows.len().saturating_sub(1)),
        }
    }

    fn set_filter(&mut self, text: Option<String>) {
        let key = self.key_at_cursor();
        self.filter = text.filter(|text| !text.is_empty());
        self.rebuild(key);
    }
}

impl FilterLine for ConnectionsView {
    fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    fn editing_filter(&self) -> bool {
        self.editing_filter
    }

    fn start_filter(&mut self) {
        self.editing_filter = true;
    }

    fn finish_filter(&mut self) {
        self.editing_filter = false;
    }

    fn type_filter(&mut self, character: char) {
        let mut text = self.filter.clone().unwrap_or_default();
        text.push(character);
        self.set_filter(Some(text));
    }

    fn erase_filter(&mut self) {
        let mut text = self.filter.clone().unwrap_or_default();
        text.pop();
        self.set_filter(Some(text));
    }

    fn clear_filter(&mut self) {
        self.editing_filter = false;
        self.set_filter(None);
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }
}

#[cfg(test)]
mod tests;
