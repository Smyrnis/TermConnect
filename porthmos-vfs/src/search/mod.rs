use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::mpsc;

use crate::{Entry, FileKind, FileSystem};

pub const MAX_SEARCH_DEPTH: usize = 16;
pub const MAX_SEARCH_RESULTS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub root: PathBuf,
    pub pattern: String,
    pub max_depth: usize,
    pub max_results: usize,
}

impl SearchQuery {
    pub fn new(root: PathBuf, pattern: String) -> Self {
        Self { root, pattern, max_depth: MAX_SEARCH_DEPTH, max_results: MAX_SEARCH_RESULTS }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEvent {
    Found(Entry),
    Done { truncated: bool },
    Failed(String),
}

pub type SearchSender = mpsc::UnboundedSender<SearchEvent>;

pub fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    match_from(&pattern, &name)
}

fn match_from(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;

    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((star_p, star_n)) = star {
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, n));
        } else {
            return false;
        }
    }

    while pattern.get(p) == Some(&'*') {
        p += 1;
    }

    p == pattern.len()
}

pub async fn walk_search<F: FileSystem + ?Sized>(
    fs: &F, query: SearchQuery, tx: SearchSender, cancel: Arc<AtomicBool>,
) {
    let SearchQuery { root, pattern, max_depth, max_results } = query;
    let mut found = 0usize;
    let mut truncated = false;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root, 0)];
    let mut is_root = true;

    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if depth > max_depth {
            continue;
        }

        let items = match fs.read_dir(&dir).await {
            Ok(items) => items,
            Err(err) => {
                if is_root {
                    let _ = tx.send(SearchEvent::Failed(err.to_string()));
                    return;
                }
                continue;
            }
        };
        is_root = false;

        for item in items {
            if cancel.load(Ordering::Relaxed) {
                return;
            }

            if found >= max_results {
                truncated = true;
                break;
            }

            let is_dir = item.metadata.kind == FileKind::Dir;

            if glob_match(&pattern, &item.name) {
                let _ = tx.send(SearchEvent::Found(Entry {
                    name: item.name.clone(),
                    path: item.path.clone(),
                    is_dir,
                    size: item.metadata.size,
                    permissions: None,
                }));
                found += 1;
            }

            if is_dir {
                stack.push((item.path, depth + 1));
            }
        }

        if found >= max_results {
            truncated = true;
            break;
        }
    }

    let _ = tx.send(SearchEvent::Done { truncated });
}

#[cfg(test)]
mod tests;
