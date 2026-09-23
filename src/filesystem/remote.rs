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

        let is_dir = if metadata.is_symlink() { sftp.metadata(&entry_path).await.map(|resolved| resolved.is_dir()).unwrap_or(false) } else { metadata.is_dir() };

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
