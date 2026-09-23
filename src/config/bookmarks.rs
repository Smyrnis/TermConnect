use std::{
    fs,
    path::{Path, PathBuf},
};

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
        if index < self.0.len() { Some(self.0.remove(index)) } else { None }
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
        Err(err) => Ok((Bookmarks::default(), vec![super::StartupWarning(format!("failed to parse bookmarks.toml: {err}"))])),
    }
}

pub(crate) fn save_to(path: &Path, bookmarks: &Bookmarks) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = {
        let mut name = path.as_os_str().to_owned();
        name.push(".tmp");
        PathBuf::from(name)
    };
    let file = BookmarksFile { bookmark: bookmarks.0.clone() };
    fs::write(&temp_path, toml::to_string_pretty(&file)?)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/config/bookmarks_test.rs"]
mod tests;
