use std::path::PathBuf;

use anyhow::Result;
use futures_util::future::BoxFuture;
use russh_sftp::client::SftpSession;

use super::Entry;

pub async fn list(sftp: &SftpSession, path: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();

    for dir_entry in sftp.read_dir(path).await? {
        let metadata = dir_entry.metadata();
        let name = dir_entry.file_name();
        let entry_path = join(path, &name);

        // `read_dir`'s attributes are lstat-based, so a symlink to a
        // directory reports `is_dir() == false` and the UI can't navigate
        // into it. Resolve it with a follow-up `stat` so it behaves like
        // the directory it points to; a broken symlink (the stat fails)
        // just falls back to the lstat result.
        let is_dir = if metadata.is_symlink() {
            sftp.metadata(&entry_path).await.map(|resolved| resolved.is_dir()).unwrap_or(false)
        } else {
            metadata.is_dir()
        };

        entries.push(Entry { path: PathBuf::from(entry_path), name, is_dir, size: metadata.len(), permissions: metadata.permissions });
    }

    Ok(entries)
}

pub async fn create_directory(sftp: &SftpSession, path: &str) -> Result<()> {
    sftp.create_dir(path).await?;
    Ok(())
}

pub async fn rename(sftp: &SftpSession, from: &str, to: &str) -> Result<()> {
    rename_overwriting(sftp, from, to).await
}

/// Renames `from` to `to`, overwriting `to` if it already exists. Plain
/// SFTP v3 `rename` refuses to overwrite an existing target, and the
/// `russh-sftp` client has no `posix-rename@openssh.com` extension support
/// to request one — but `filesystem::local::rename`'s `fs::rename` does
/// overwrite unconditionally on POSIX, so the remote side matches that.
///
/// Falls back to swapping the existing target out of the way first
/// (`to` -> `to.bak`), retrying the rename, then removing the backup on
/// success or restoring it on failure — `to` is never left missing. If
/// the first rename fails for a reason other than "target exists" (e.g.
/// permission denied), the swap-out step fails too and that error
/// propagates, with nothing having changed.
pub async fn rename_overwriting(sftp: &SftpSession, from: &str, to: &str) -> Result<()> {
    if sftp.rename(from, to).await.is_ok() {
        return Ok(());
    }

    let backup = format!("{to}.bak");
    sftp.rename(to, &backup).await?;

    match sftp.rename(from, to).await {
        Ok(()) => {
            let _ = sftp.remove_file(&backup).await;
            Ok(())
        }
        Err(err) => {
            let _ = sftp.rename(&backup, to).await;
            Err(err.into())
        }
    }
}

/// Deletes a file, or a directory and everything in it (SFTP's own
/// `remove_dir` refuses non-empty directories, so a recursive delete
/// requires listing and removing children first). Uses `symlink_metadata`
/// (lstat) rather than `metadata` (stat) so a symlink to a directory is
/// unlinked itself, matching `filesystem::local::delete`'s existing
/// lstat-based behavior — `metadata` would follow the link and recurse
/// into the target directory's contents instead.
pub async fn delete(sftp: &SftpSession, path: &str) -> Result<()> {
    let metadata = sftp.symlink_metadata(path).await?;

    if metadata.is_dir() {
        remove_dir_recursive(sftp, path).await
    } else {
        sftp.remove_file(path).await?;
        Ok(())
    }
}

fn remove_dir_recursive<'a>(sftp: &'a SftpSession, path: &'a str) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        let children: Vec<_> = sftp.read_dir(path).await?.collect();

        for child in children {
            let child_path = join(path, &child.file_name());
            if child.metadata().is_dir() {
                remove_dir_recursive(sftp, &child_path).await?;
            } else {
                sftp.remove_file(&child_path).await?;
            }
        }

        sftp.remove_dir(path).await?;
        Ok(())
    })
}

pub(crate) fn join(parent: &str, name: &str) -> String {
    if parent.ends_with('/') { format!("{parent}{name}") } else { format!("{parent}/{name}") }
}

#[cfg(test)]
#[path = "../../tests/filesystem/remote_test.rs"]
mod tests;
