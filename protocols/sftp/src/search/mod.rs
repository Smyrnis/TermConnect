use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use russh::{ChannelMsg, client::Handle};
use russh_sftp::client::SftpSession;
use tokio::sync::mpsc;

use porthmos_vfs::Entry;
pub use porthmos_vfs::SearchEvent;
use porthmos_vfs::{SearchQuery, glob_match, join_remote, path_to_remote_string};

use crate::client::PorthmosHandler;

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

pub async fn search_remote(
    handle: &Handle<PorthmosHandler>, sftp: &SftpSession, query: SearchQuery, tx: mpsc::UnboundedSender<SearchEvent>,
    cancel: Arc<AtomicBool>,
) {
    let root = path_to_remote_string(&query.root);
    let SearchQuery { pattern, max_depth, max_results, .. } = query;
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

async fn run_find(
    handle: &Handle<PorthmosHandler>, root: &str, pattern: &str, max_depth: usize, cancel: &Arc<AtomicBool>,
) -> Option<Vec<String>> {
    let mut channel = handle.channel_open_session().await.ok()?;
    let command = format!("find {} -maxdepth {max_depth} -iname {}", shell_quote(root), shell_quote(pattern));
    channel.exec(true, command.into_bytes()).await.ok()?;

    let mut output = Vec::new();
    let mut exit_ok = false;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }

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
            let path = join_remote(&dir, &name);

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
mod tests;
