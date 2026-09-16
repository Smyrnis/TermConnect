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

        entries.push(Entry {
            path: PathBuf::from(join(path, &name)),
            name,
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            permissions: metadata.permissions,
        });
    }

    Ok(entries)
}

pub async fn create_directory(sftp: &SftpSession, path: &str) -> Result<()> {
    sftp.create_dir(path).await?;
    Ok(())
}

pub async fn rename(sftp: &SftpSession, from: &str, to: &str) -> Result<()> {
    sftp.rename(from, to).await?;
    Ok(())
}

/// Deletes a file, or a directory and everything in it (SFTP's own
/// `remove_dir` refuses non-empty directories, so a recursive delete
/// requires listing and removing children first).
pub async fn delete(sftp: &SftpSession, path: &str) -> Result<()> {
    let metadata = sftp.metadata(path).await?;

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
    if parent.ends_with('/') {
        format!("{parent}{name}")
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_adds_a_separator_when_missing() {
        assert_eq!(join("/home/user", "file.txt"), "/home/user/file.txt");
    }

    #[test]
    fn join_does_not_double_the_separator() {
        assert_eq!(join("/home/user/", "file.txt"), "/home/user/file.txt");
    }

    #[test]
    fn join_handles_root() {
        assert_eq!(join("/", "etc"), "/etc");
    }
}
