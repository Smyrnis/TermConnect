use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use russh::ChannelMsg;
use russh::client::Handle;
use russh_sftp::client::SftpSession;
use tokio::sync::mpsc;

use crate::connection::client::TermConnectHandler;
use crate::filesystem::remote::join;

use super::Entry;

const MAX_DEPTH: usize = 16;
const MAX_RESULTS: usize = 1000;

pub enum SearchEvent {
    Found(Entry),
    Done { truncated: bool },
    Failed(String),
}

/// Matches `*` (any run of characters) and `?` (exactly one character)
/// against `name`, case-sensitively — not a general glob implementation,
/// just the two wildcards file-name searching needs.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    match_from(&pattern, &name)
}

fn match_from(pattern: &[char], name: &[char]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some('*'), _) => {
            match_from(&pattern[1..], name) || (!name.is_empty() && match_from(pattern, &name[1..]))
        }
        (Some('?'), Some(_)) => match_from(&pattern[1..], &name[1..]),
        (Some(p), Some(n)) if p == n => match_from(&pattern[1..], &name[1..]),
        _ => false,
    }
}

pub async fn search_local(
    root: PathBuf,
    pattern: String,
    tx: mpsc::UnboundedSender<SearchEvent>,
    cancel: Arc<AtomicBool>,
) {
    search_local_with_limits(root, pattern, tx, cancel, MAX_DEPTH, MAX_RESULTS).await;
}

/// Depth-limited, cancellable, capped recursive walk. Split from
/// `search_local` so tests can exercise the depth/cap boundaries with
/// small numbers instead of `MAX_DEPTH`/`MAX_RESULTS`.
async fn search_local_with_limits(
    root: PathBuf,
    pattern: String,
    tx: mpsc::UnboundedSender<SearchEvent>,
    cancel: Arc<AtomicBool>,
    max_depth: usize,
    max_results: usize,
) {
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

        let mut read_dir = match tokio::fs::read_dir(&dir).await {
            Ok(read_dir) => read_dir,
            Err(err) => {
                if is_root {
                    let _ = tx.send(SearchEvent::Failed(err.to_string()));
                    return;
                }
                continue;
            }
        };
        is_root = false;

        loop {
            if cancel.load(Ordering::Relaxed) {
                return;
            }

            let dir_entry = match read_dir.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(_) => break,
            };

            if found >= max_results {
                truncated = true;
                break;
            }

            let name = dir_entry.file_name().to_string_lossy().into_owned();
            let path = dir_entry.path();
            let is_dir = dir_entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false);

            if glob_match(&pattern, &name) {
                let size = dir_entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                let _ = tx.send(SearchEvent::Found(Entry {
                    name,
                    path: path.clone(),
                    is_dir,
                    size,
                    permissions: None,
                }));
                found += 1;
            }

            if is_dir {
                stack.push((path, depth + 1));
            }
        }

        if found >= max_results {
            truncated = true;
            break;
        }
    }

    let _ = tx.send(SearchEvent::Done { truncated });
}

/// Wraps `value` in single quotes for a POSIX shell command line, escaping
/// any embedded `'` — the standard way to pass an arbitrary string as one
/// shell argument without it being interpreted.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

pub async fn search_remote(
    handle: &Handle<TermConnectHandler>,
    sftp: &SftpSession,
    root: String,
    pattern: String,
    tx: mpsc::UnboundedSender<SearchEvent>,
    cancel: Arc<AtomicBool>,
) {
    search_remote_with_limits(
        handle,
        sftp,
        root,
        pattern,
        tx,
        cancel,
        MAX_DEPTH,
        MAX_RESULTS,
    )
    .await;
}

async fn search_remote_with_limits(
    handle: &Handle<TermConnectHandler>,
    sftp: &SftpSession,
    root: String,
    pattern: String,
    tx: mpsc::UnboundedSender<SearchEvent>,
    cancel: Arc<AtomicBool>,
    max_depth: usize,
    max_results: usize,
) {
    match run_find(handle, &root, &pattern, max_depth).await {
        Some(paths) => {
            let mut found = 0usize;
            let mut truncated = false;

            for path in paths {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                if found >= max_results {
                    truncated = true;
                    break;
                }

                if let Ok(metadata) = sftp.metadata(&path).await {
                    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                    let _ = tx.send(SearchEvent::Found(Entry {
                        name,
                        path: PathBuf::from(&path),
                        is_dir: metadata.is_dir(),
                        size: metadata.len(),
                        permissions: metadata.permissions,
                    }));
                    found += 1;
                }
            }

            let _ = tx.send(SearchEvent::Done { truncated });
        }
        None => {
            search_remote_walk(sftp, root, pattern, tx, cancel, max_depth, max_results).await;
        }
    }
}

/// Runs `find <root> -maxdepth <max_depth> -iname <pattern>` over an SSH
/// exec channel — the only remote command execution in TermConnect, and it
/// only ever runs on an explicit search. Returns `None` (triggering the
/// SFTP-walk fallback) if the channel can't be opened, `exec` fails, or the
/// command exits non-zero.
async fn run_find(
    handle: &Handle<TermConnectHandler>,
    root: &str,
    pattern: &str,
    max_depth: usize,
) -> Option<Vec<String>> {
    let mut channel = handle.channel_open_session().await.ok()?;
    let command = format!(
        "find {} -maxdepth {max_depth} -iname {}",
        shell_quote(root),
        shell_quote(pattern)
    );
    channel.exec(true, command.into_bytes()).await.ok()?;

    let mut output = Vec::new();
    let mut exit_ok = false;

    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { data } => output.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status } => exit_ok = exit_status == 0,
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }

    if !exit_ok {
        return None;
    }

    let text = String::from_utf8_lossy(&output);
    Some(
        text.lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// Depth-limited, cancellable, capped SFTP `read_dir` walk — the fallback
/// when `find` isn't available, mirroring `search_local`'s shape.
async fn search_remote_walk(
    sftp: &SftpSession,
    root: String,
    pattern: String,
    tx: mpsc::UnboundedSender<SearchEvent>,
    cancel: Arc<AtomicBool>,
    max_depth: usize,
    max_results: usize,
) {
    let mut found = 0usize;
    let mut truncated = false;
    let mut stack: Vec<(String, usize)> = vec![(root, 0)];

    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if depth > max_depth {
            continue;
        }

        let entries = match sftp.read_dir(&dir).await {
            Ok(read_dir) => read_dir.collect::<Vec<_>>(),
            Err(_) => continue,
        };

        for dir_entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            if found >= max_results {
                truncated = true;
                break;
            }

            let name = dir_entry.file_name();
            let metadata = dir_entry.metadata();
            let path = join(&dir, &name);

            if glob_match(&pattern, &name) {
                let _ = tx.send(SearchEvent::Found(Entry {
                    name: name.clone(),
                    path: PathBuf::from(&path),
                    is_dir: metadata.is_dir(),
                    size: metadata.len(),
                    permissions: metadata.permissions,
                }));
                found += 1;
            }

            if metadata.is_dir() {
                stack.push((path, depth + 1));
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
mod tests {
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
        // Dangerous outside quotes, inert once wrapped — quoting the whole
        // argument is the defense, not denying individual characters.
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
    }

    #[tokio::test]
    async fn search_local_stops_promptly_when_already_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.log"), b"x").unwrap();
        let cancel = Arc::new(AtomicBool::new(true));

        let (tx, rx) = mpsc::unbounded_channel();
        search_local_with_limits(
            dir.path().to_path_buf(),
            "*.log".to_string(),
            tx,
            cancel,
            16,
            1000,
        )
        .await;

        let (found, _) = drain(rx).await;
        assert!(found.is_empty());
    }
}
