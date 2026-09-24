use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use tokio::sync::mpsc;

use super::*;
use crate::{DirItem, FileKind, FileSystem, Metadata, ProtocolError, Reader, Writer};

struct LocalTestFs;

#[async_trait::async_trait]
impl FileSystem for LocalTestFs {
    async fn list(&self, _dir: &Path) -> Result<Vec<Entry>, ProtocolError> {
        unimplemented!()
    }

    async fn read_dir(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError> {
        let mut items = Vec::new();
        for dir_entry in fs::read_dir(dir)? {
            let dir_entry = dir_entry?;
            let metadata = fs::symlink_metadata(dir_entry.path())?;
            let kind = if metadata.file_type().is_symlink() {
                FileKind::Symlink
            } else if metadata.is_dir() {
                FileKind::Dir
            } else {
                FileKind::File
            };
            items.push(DirItem {
                name: dir_entry.file_name().to_string_lossy().into_owned(),
                path: dir_entry.path(),
                metadata: Metadata { size: metadata.len(), modified: None, kind, permissions: None },
            });
        }
        Ok(items)
    }

    async fn stat(&self, _path: &Path) -> Result<Metadata, ProtocolError> {
        unimplemented!()
    }

    async fn create_dir(&self, _path: &Path) -> Result<(), ProtocolError> {
        unimplemented!()
    }

    async fn rename(&self, _from: &Path, _to: &Path) -> Result<(), ProtocolError> {
        unimplemented!()
    }

    async fn remove_file(&self, _path: &Path) -> Result<(), ProtocolError> {
        unimplemented!()
    }

    async fn delete(&self, _path: &Path) -> Result<(), ProtocolError> {
        unimplemented!()
    }

    async fn home(&self) -> Result<PathBuf, ProtocolError> {
        unimplemented!()
    }

    async fn open_read(&self, _path: &Path, _offset: u64) -> Result<Reader, ProtocolError> {
        unimplemented!()
    }

    async fn open_write(&self, _path: &Path, _offset: u64) -> Result<Writer, ProtocolError> {
        unimplemented!()
    }

    async fn search(&self, query: SearchQuery, tx: SearchSender, cancel: Arc<AtomicBool>) {
        walk_search(self, query, tx, cancel).await
    }
}

#[test]
fn glob_match_supports_star_and_question_wildcards() {
    assert!(glob_match("*.log", "error.log"));
    assert!(!glob_match("*.log", "error.txt"));
    assert!(glob_match("file?.txt", "file1.txt"));
    assert!(!glob_match("file?.txt", "file12.txt"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("exact.txt", "exact.txt"));
    assert!(!glob_match("exact.txt", "other.txt"));
    assert!(glob_match("", ""));
    assert!(!glob_match("", "nonempty"));
}

#[test]
fn glob_match_does_not_blow_up_on_pathological_backtracking_patterns() {
    let pattern = "*a".repeat(30) + "*b";
    let name = "a".repeat(40);

    let start = std::time::Instant::now();
    let matched = glob_match(&pattern, &name);
    let elapsed = start.elapsed();

    assert!(!matched);
    assert!(elapsed < std::time::Duration::from_secs(5), "took {elapsed:?}");
}

#[test]
fn glob_match_is_case_insensitive() {
    assert!(glob_match("*.LOG", "error.log"));
    assert!(glob_match("*.log", "ERROR.LOG"));
    assert!(glob_match("File?.txt", "file1.TXT"));
    assert!(glob_match("EXACT.txt", "exact.TXT"));
}

async fn drain(mut rx: mpsc::UnboundedReceiver<SearchEvent>) -> (Vec<Entry>, bool) {
    let mut found = Vec::new();
    let mut truncated = false;
    while let Some(event) = rx.recv().await {
        match event {
            SearchEvent::Found(entry) => found.push(entry),
            SearchEvent::Done { truncated: t } => {
                truncated = t;
                break;
            }
            SearchEvent::Failed(_) => break,
        }
    }
    (found, truncated)
}

#[tokio::test]
async fn search_local_finds_matching_files_recursively() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("a.log"), b"x").unwrap();
    fs::write(dir.path().join("sub/b.log"), b"x").unwrap();
    fs::write(dir.path().join("c.txt"), b"x").unwrap();

    let (tx, rx) = mpsc::unbounded_channel();
    walk_search(
        &LocalTestFs,
        SearchQuery { root: dir.path().to_path_buf(), pattern: "*.log".to_string(), max_depth: 16, max_results: 1000 },
        tx,
        Arc::new(AtomicBool::new(false)),
    )
    .await;

    let (found, truncated) = drain(rx).await;
    assert!(!truncated);
    let mut names: Vec<String> = found.into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, vec!["a.log".to_string(), "b.log".to_string()]);
}

#[tokio::test]
async fn search_local_respects_the_depth_limit() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("target.log"), b"x").unwrap();

    let (tx, rx) = mpsc::unbounded_channel();
    walk_search(
        &LocalTestFs,
        SearchQuery { root: dir.path().to_path_buf(), pattern: "*.log".to_string(), max_depth: 1, max_results: 1000 },
        tx,
        Arc::new(AtomicBool::new(false)),
    )
    .await;

    let (found, _) = drain(rx).await;
    assert!(found.is_empty());
}

#[tokio::test]
async fn search_local_reports_truncation_at_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        fs::write(dir.path().join(format!("{i}.log")), b"x").unwrap();
    }

    let (tx, rx) = mpsc::unbounded_channel();
    walk_search(
        &LocalTestFs,
        SearchQuery { root: dir.path().to_path_buf(), pattern: "*.log".to_string(), max_depth: 16, max_results: 3 },
        tx,
        Arc::new(AtomicBool::new(false)),
    )
    .await;

    let (found, truncated) = drain(rx).await;
    assert_eq!(found.len(), 3);
    assert!(truncated);
}

#[tokio::test]
async fn search_local_stops_promptly_when_already_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.log"), b"x").unwrap();
    let cancel = Arc::new(AtomicBool::new(true));

    let (tx, rx) = mpsc::unbounded_channel();
    walk_search(
        &LocalTestFs,
        SearchQuery { root: dir.path().to_path_buf(), pattern: "*.log".to_string(), max_depth: 16, max_results: 1000 },
        tx,
        cancel,
    )
    .await;

    let (found, _) = drain(rx).await;
    assert!(found.is_empty());
}

#[tokio::test]
async fn search_local_reports_a_root_that_cannot_be_read() {
    let (tx, rx) = mpsc::unbounded_channel();
    walk_search(
        &LocalTestFs,
        SearchQuery::new(PathBuf::from("/definitely/missing/root"), "*".to_string()),
        tx,
        Arc::new(AtomicBool::new(false)),
    )
    .await;
    let mut rx = rx;
    assert!(matches!(rx.recv().await, Some(SearchEvent::Failed(_))));
}
