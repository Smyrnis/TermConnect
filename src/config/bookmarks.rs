use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Bookmark {
    pub label: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bookmarks(Vec<Bookmark>);

impl Bookmarks {
    pub fn iter(&self) -> impl Iterator<Item = &Bookmark> {
        self.0.iter()
    }

    // Exercised by tests only for now; kept as normal collection API
    // rather than test-gated, since a caller with a `Bookmarks` in hand
    // reasonably expects `len`/`is_empty` alongside `iter`/`get`.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Bookmark> {
        self.0.get(index)
    }

    pub fn add(&mut self, bookmark: Bookmark) {
        self.0.push(bookmark);
    }

    pub fn remove(&mut self, index: usize) -> Option<Bookmark> {
        if index < self.0.len() {
            Some(self.0.remove(index))
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct BookmarksFile {
    #[serde(default, rename = "bookmark")]
    bookmark: Vec<Bookmark>,
}

pub fn bookmarks_path() -> Result<PathBuf> {
    Ok(super::config_dir()?.join("bookmarks.toml"))
}

/// Loads bookmarks. Same never-fail contract as `config::load`: a missing
/// file is empty with no warnings, a malformed one recovers to empty with
/// one warning.
pub fn load() -> Result<(Bookmarks, Vec<super::StartupWarning>)> {
    load_from(&bookmarks_path()?)
}

fn load_from(path: &Path) -> Result<(Bookmarks, Vec<super::StartupWarning>)> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Bookmarks::default(), Vec::new()));
        }
        Err(err) => return Err(err.into()),
    };

    match toml::from_str::<BookmarksFile>(&contents) {
        Ok(file) => Ok((Bookmarks(file.bookmark), Vec::new())),
        Err(err) => Ok((
            Bookmarks::default(),
            vec![super::StartupWarning(format!(
                "failed to parse bookmarks.toml: {err}"
            ))],
        )),
    }
}

/// Rewrites the whole file — small and human-editable, so there's no need
/// for incremental writes.
pub(crate) fn save_to(path: &Path, bookmarks: &Bookmarks) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = BookmarksFile {
        bookmark: bookmarks.0.clone(),
    };
    fs::write(path, toml::to_string_pretty(&file)?)?;
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/config/bookmarks_test.rs"]
mod tests;
