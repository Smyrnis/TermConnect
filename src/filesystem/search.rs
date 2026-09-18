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
/// against `name`, case-insensitively (matching remote `find -iname`'s
/// behavior, so local search, the `find`-based remote search, and the
/// SFTP-walk remote fallback all agree on the same typed pattern) — not a
/// general glob implementation, just the two wildcards file-name searching
/// needs.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    match_from(&pattern, &name)
}

fn match_from(pattern: &[char], name: &[char]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some('*'), _) => match_from(&pattern[1..], name) || (!name.is_empty() && match_from(pattern, &name[1..])),
        (Some('?'), Some(_)) => match_from(&pattern[1..], &name[1..]),
        (Some(p), Some(n)) if p == n => match_from(&pattern[1..], &name[1..]),
        _ => false,
    }
}

pub async fn search_local(
    root: PathBuf, pattern: String, tx: mpsc::UnboundedSender<SearchEvent>, cancel: Arc<AtomicBool>,
) {
    search_local_with_limits(root, pattern, tx, cancel, MAX_DEPTH, MAX_RESULTS).await;
}

/// Depth-limited, cancellable, capped recursive walk. Split from
/// `search_local` so tests can exercise the depth/cap boundaries with
/// small numbers instead of `MAX_DEPTH`/`MAX_RESULTS`.
async fn search_local_with_limits(
    root: PathBuf, pattern: String, tx: mpsc::UnboundedSender<SearchEvent>, cancel: Arc<AtomicBool>, max_depth: usize,
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
            let is_dir = dir_entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);

            if glob_match(&pattern, &name) {
                let size = dir_entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                let _ =
                    tx.send(SearchEvent::Found(Entry { name, path: path.clone(), is_dir, size, permissions: None }));
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
    handle: &Handle<TermConnectHandler>, sftp: &SftpSession, root: String, pattern: String,
    tx: mpsc::UnboundedSender<SearchEvent>, cancel: Arc<AtomicBool>,
) {
    search_remote_with_limits(handle, sftp, root, pattern, tx, cancel, MAX_DEPTH, MAX_RESULTS).await;
}

/// `search_remote`'s inner implementation, parameterized on `max_depth`/
/// `max_results` so tests can exercise the truncation/depth-limit behavior
/// without waiting on the real (much larger) `MAX_DEPTH`/`MAX_RESULTS`.
#[allow(clippy::too_many_arguments)]
async fn search_remote_with_limits(
    handle: &Handle<TermConnectHandler>, sftp: &SftpSession, root: String, pattern: String,
    tx: mpsc::UnboundedSender<SearchEvent>, cancel: Arc<AtomicBool>, max_depth: usize, max_results: usize,
) {
    match run_find(handle, &root, &pattern, max_depth, &cancel).await {
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
    handle: &Handle<TermConnectHandler>, root: &str, pattern: &str, max_depth: usize, cancel: &Arc<AtomicBool>,
) -> Option<Vec<String>> {
    let mut channel = handle.channel_open_session().await.ok()?;
    let command = format!("find {} -maxdepth {max_depth} -iname {}", shell_quote(root), shell_quote(pattern));
    channel.exec(true, command.into_bytes()).await.ok()?;

    let mut output = Vec::new();
    let mut exit_ok = false;

    loop {
        if cancel.load(Ordering::Relaxed) {
            // Dropping `channel` closes the SSH exec channel, ending the
            // remote `find` process rather than letting it run to
            // completion after the caller has stopped listening.
            return None;
        }

        // Race the next channel message against a short poll interval so a
        // cancellation flag flip is noticed promptly even while `find` is
        // silently still running remotely (no data arriving to wake us).
        let msg = tokio::select! {
            msg = channel.wait() => msg,
            () = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
        };

        match msg {
            Some(ChannelMsg::Data { data }) => output.extend_from_slice(&data),
            Some(ChannelMsg::ExitStatus { exit_status }) => exit_ok = exit_status == 0,
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
            Some(_) => {}
            None => break,
        }
    }

    if !exit_ok {
        return None;
    }

    let text = String::from_utf8_lossy(&output);
    Some(text.lines().filter(|line| !line.is_empty()).map(str::to_string).collect())
}

/// Depth-limited, cancellable, capped SFTP `read_dir` walk — the fallback
/// when `find` isn't available, mirroring `search_local`'s shape.
async fn search_remote_walk(
    sftp: &SftpSession, root: String, pattern: String, tx: mpsc::UnboundedSender<SearchEvent>, cancel: Arc<AtomicBool>,
    max_depth: usize, max_results: usize,
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
#[path = "../../tests/filesystem/search_test.rs"]
mod tests;
