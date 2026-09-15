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

    pub fn len(&self) -> usize {
        self.0.len()
    }

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
pub fn save(bookmarks: &Bookmarks) -> Result<()> {
    save_to(&bookmarks_path()?, bookmarks)
}

fn save_to(path: &Path, bookmarks: &Bookmarks) -> Result<()> {
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
mod tests {
    use super::*;
    use std::fs;

    fn sample() -> Bookmark {
        Bookmark {
            label: "projects".to_string(),
            path: PathBuf::from("/home/user/projects"),
            host: None,
        }
    }

    #[test]
    fn add_and_remove_manage_the_list() {
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(sample());
        assert_eq!(bookmarks.len(), 1);

        let removed = bookmarks.remove(0).unwrap();
        assert_eq!(removed.label, "projects");
        assert!(bookmarks.is_empty());
    }

    #[test]
    fn load_from_a_missing_file_returns_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bookmarks.toml");

        let (bookmarks, warnings) = load_from(&path).unwrap();

        assert!(bookmarks.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn save_then_load_round_trips_including_a_remote_bookmark() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bookmarks.toml");
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(sample());
        bookmarks.add(Bookmark {
            label: "nginx conf".to_string(),
            path: PathBuf::from("/etc/nginx"),
            host: Some("production".to_string()),
        });

        save_to(&path, &bookmarks).unwrap();
        let (loaded, warnings) = load_from(&path).unwrap();

        assert!(warnings.is_empty());
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(1).unwrap().host, Some("production".to_string()));
    }

    #[test]
    fn load_from_recovers_to_empty_on_malformed_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bookmarks.toml");
        fs::write(&path, "not [ valid").unwrap();

        let (bookmarks, warnings) = load_from(&path).unwrap();

        assert!(bookmarks.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
