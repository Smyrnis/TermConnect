use super::*;
use std::fs;
use std::sync::Arc;
use tokio::sync::mpsc;

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
    search_local_with_limits(
        dir.path().to_path_buf(),
        "*.log".to_string(),
        tx,
        Arc::new(AtomicBool::new(false)),
        16,
        1000,
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
    search_local_with_limits(
        dir.path().to_path_buf(),
        "*.log".to_string(),
        tx,
        Arc::new(AtomicBool::new(false)),
        1,
        1000,
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
    search_local_with_limits(
        dir.path().to_path_buf(),
        "*.log".to_string(),
        tx,
        Arc::new(AtomicBool::new(false)),
        16,
        3,
    )
    .await;

    let (found, truncated) = drain(rx).await;
    assert_eq!(found.len(), 3);
    assert!(truncated);
}

#[test]
fn shell_quote_wraps_plain_text_in_single_quotes() {
    assert_eq!(shell_quote("simple"), "'simple'");
}

#[test]
fn shell_quote_escapes_embedded_single_quotes() {
    assert_eq!(shell_quote("it's here"), "'it'\\''s here'");
}

#[test]
fn shell_quote_preserves_other_shell_metacharacters_literally_inside_quotes() {
    assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
}

#[tokio::test]
async fn search_local_stops_promptly_when_already_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.log"), b"x").unwrap();
    let cancel = Arc::new(AtomicBool::new(true));

    let (tx, rx) = mpsc::unbounded_channel();
    search_local_with_limits(dir.path().to_path_buf(), "*.log".to_string(), tx, cancel, 16, 1000).await;

    let (found, _) = drain(rx).await;
    assert!(found.is_empty());
}
