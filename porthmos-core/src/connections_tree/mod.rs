use std::collections::{BTreeMap, HashSet};

use crate::profiles::ConnectionEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    Group { path: String, name: String, depth: usize, count: usize, expanded: bool },
    Connection { index: usize, depth: usize },
}

#[derive(Default)]
struct Node {
    groups: BTreeMap<(String, String), Node>,
    connections: Vec<usize>,
    count: usize,
}

pub fn matches(entry: &ConnectionEntry, filter: &str) -> bool {
    let filter = filter.trim();
    if let Some(tag) = filter.strip_prefix('#') {
        let tag = tag.trim();
        return tag.is_empty() || entry.tags.iter().any(|kept| kept.to_lowercase() == tag.to_lowercase());
    }
    let needle = filter.to_lowercase();
    entry.name.to_lowercase().contains(&needle)
        || entry.host.to_lowercase().contains(&needle)
        || entry.group.as_deref().is_some_and(|group| group.to_lowercase().contains(&needle))
        || entry.tags.iter().any(|tag| tag.to_lowercase().contains(&needle))
}

pub fn active_filter(filter: Option<&str>) -> Option<&str> {
    filter.filter(|text| !text.trim().trim_start_matches('#').trim().is_empty())
}

pub fn match_count(entries: &[ConnectionEntry], filter: Option<&str>) -> usize {
    entries.iter().filter(|entry| filter.is_none_or(|filter| matches(entry, filter))).count()
}

pub fn rows(entries: &[ConnectionEntry], expanded: &HashSet<String>, filter: Option<&str>) -> Vec<TreeRow> {
    let filter = active_filter(filter);
    let mut root = Node::default();
    for (index, entry) in entries.iter().enumerate() {
        if filter.is_some_and(|filter| !matches(entry, filter)) {
            continue;
        }
        let mut node = &mut root;
        node.count += 1;
        if let Some(group) = &entry.group {
            for segment in group.split('/') {
                node = node.groups.entry((segment.to_lowercase(), segment.to_string())).or_default();
                node.count += 1;
            }
        }
        node.connections.push(index);
    }
    let mut rows = Vec::new();
    push_rows(&root, "", 0, expanded, filter.is_some(), &mut rows);
    rows
}

fn push_rows(
    node: &Node, prefix: &str, depth: usize, expanded: &HashSet<String>, open_all: bool, rows: &mut Vec<TreeRow>,
) {
    for ((_, name), child) in &node.groups {
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let is_open = open_all || expanded.contains(&path);
        rows.push(TreeRow::Group {
            path: path.clone(),
            name: name.clone(),
            depth,
            count: child.count,
            expanded: is_open,
        });
        if is_open {
            push_rows(child, &path, depth + 1, expanded, open_all, rows);
        }
    }
    rows.extend(node.connections.iter().map(|&index| TreeRow::Connection { index, depth }));
}

#[cfg(test)]
mod tests;
