use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::persist::{Loaded, read_or_set_aside, toml_problem, write_atomic};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Bookmark {
    pub label: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bookmarks {
    items: Vec<Bookmark>,
    protected: bool,
}

impl Bookmarks {
    pub fn iter(&self) -> impl Iterator<Item = &Bookmark> {
        self.items.iter()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Bookmark> {
        self.items.get(index)
    }

    pub fn add(&mut self, bookmark: Bookmark) {
        self.items.push(bookmark);
    }

    pub fn is_protected(&self) -> bool {
        self.protected
    }

    #[cfg(test)]
    pub(crate) fn protected_for_test(items: Vec<Bookmark>) -> Self {
        Self { items, protected: true }
    }

    pub fn remove(&mut self, index: usize) -> Option<Bookmark> {
        if index < self.items.len() { Some(self.items.remove(index)) } else { None }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct BookmarksFile {
    #[serde(default, rename = "bookmark")]
    bookmark: Vec<Bookmark>,
}

pub fn load(paths: &crate::Paths) -> Result<(Bookmarks, Vec<super::StartupWarning>)> {
    load_from(&paths.bookmarks_file())
}

fn load_from(path: &Path) -> Result<(Bookmarks, Vec<super::StartupWarning>)> {
    Ok(
        match read_or_set_aside(path, "bookmarks", |text| {
            toml::from_str::<BookmarksFile>(text).map_err(|err| toml_problem(text, &err))
        }) {
            Loaded::Missing => (Bookmarks::default(), Vec::new()),
            Loaded::Ready(file) => (Bookmarks { items: file.bookmark, protected: false }, Vec::new()),
            Loaded::SetAside { warning, protected } => {
                (Bookmarks { items: Vec::new(), protected }, vec![super::StartupWarning(warning)])
            }
        },
    )
}

pub fn save_to(path: &Path, bookmarks: &Bookmarks) -> Result<()> {
    if bookmarks.protected {
        anyhow::bail!("bookmarks.toml could not be read or set aside, so it was left alone");
    }
    let file = BookmarksFile { bookmark: bookmarks.items.clone() };
    write_atomic(path, toml::to_string_pretty(&file)?.as_bytes(), 0o600)
}

#[cfg(test)]
mod tests;
