use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use futures_util::future::BoxFuture;
use porthmos_vfs::{
    DirItem, Entry, ErrorKind, FileKind, FileSystem, Metadata, ProtocolError, Reader, SearchQuery, SearchSender,
    Writer, async_trait, join_remote, path_to_remote_string,
};
use russh_sftp::{
    client::{SftpSession, error::Error as SftpError, fs::Metadata as SftpMetadata},
    protocol::{FileAttributes, OpenFlags, StatusCode},
};
use tokio::io::AsyncSeekExt;

use porthmos_ssh::Session;

use crate::subsystem::{SFTP_MAX_CONCURRENT_WRITES, SFTP_MAX_WRITE_PACKET_LEN};

pub const SFTP_RESUME_BACKOFF_BYTES: u64 = SFTP_MAX_CONCURRENT_WRITES as u64 * SFTP_MAX_WRITE_PACKET_LEN as u64;

pub struct SftpFs {
    session: Arc<Session>,
    sftp: Arc<SftpSession>,
}

impl SftpFs {
    pub fn new(session: Arc<Session>, sftp: Arc<SftpSession>) -> Self {
        Self { session, sftp }
    }
}

pub fn sftp_error(error: SftpError) -> ProtocolError {
    let kind = match &error {
        SftpError::Status(status) if status.status_code == StatusCode::NoSuchFile => ErrorKind::NotFound,
        SftpError::Status(status) if status.status_code == StatusCode::PermissionDenied => ErrorKind::PermissionDenied,
        _ => ErrorKind::Other,
    };
    ProtocolError::new(kind, error)
}

pub fn metadata_from_sftp(metadata: &SftpMetadata) -> Metadata {
    let kind = if metadata.is_symlink() {
        FileKind::Symlink
    } else if metadata.is_dir() {
        FileKind::Dir
    } else {
        FileKind::File
    };
    Metadata { size: metadata.len(), modified: metadata.mtime.map(u64::from), kind, permissions: metadata.permissions }
}

fn remote(path: &Path) -> String {
    path_to_remote_string(path)
}

async fn rename_overwriting(sftp: &SftpSession, from: &str, to: &str) -> Result<(), SftpError> {
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
            Err(err)
        }
    }
}

fn remove_dir_recursive<'a>(sftp: &'a SftpSession, path: &'a str) -> BoxFuture<'a, Result<(), SftpError>> {
    Box::pin(async move {
        let children: Vec<_> = sftp.read_dir(path).await?.collect();

        for child in children {
            let child_path = join_remote(path, &child.file_name());
            if child.metadata().is_dir() {
                remove_dir_recursive(sftp, &child_path).await?;
            } else {
                sftp.remove_file(&child_path).await?;
            }
        }

        sftp.remove_dir(path).await
    })
}

#[async_trait]
impl FileSystem for SftpFs {
    async fn list(&self, dir: &Path) -> Result<Vec<Entry>, ProtocolError> {
        let path = remote(dir);
        let mut entries = Vec::new();

        for dir_entry in self.sftp.read_dir(&path).await.map_err(sftp_error)? {
            let metadata = dir_entry.metadata();
            let name = dir_entry.file_name();
            let entry_path = join_remote(&path, &name);

            let is_dir = if metadata.is_symlink() {
                self.sftp.metadata(&entry_path).await.map(|resolved| resolved.is_dir()).unwrap_or(false)
            } else {
                metadata.is_dir()
            };

            entries.push(Entry {
                path: PathBuf::from(entry_path),
                name,
                is_dir,
                size: metadata.len(),
                permissions: metadata.permissions,
            });
        }

        Ok(entries)
    }

    async fn read_dir(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError> {
        let path = remote(dir);
        Ok(self
            .sftp
            .read_dir(&path)
            .await
            .map_err(sftp_error)?
            .map(|dir_entry| {
                let name = dir_entry.file_name();
                DirItem {
                    path: PathBuf::from(join_remote(&path, &name)),
                    metadata: metadata_from_sftp(&dir_entry.metadata()),
                    name,
                }
            })
            .collect())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata, ProtocolError> {
        Ok(metadata_from_sftp(&self.sftp.metadata(remote(path)).await.map_err(sftp_error)?))
    }

    async fn create_dir(&self, path: &Path) -> Result<(), ProtocolError> {
        self.sftp.create_dir(remote(path)).await.map_err(sftp_error)
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<(), ProtocolError> {
        rename_overwriting(&self.sftp, &remote(from), &remote(to)).await.map_err(sftp_error)
    }

    async fn remove_file(&self, path: &Path) -> Result<(), ProtocolError> {
        self.sftp.remove_file(remote(path)).await.map_err(sftp_error)
    }

    async fn delete(&self, path: &Path) -> Result<(), ProtocolError> {
        let path = remote(path);
        let metadata = self.sftp.symlink_metadata(&path).await.map_err(sftp_error)?;

        if metadata.is_dir() {
            remove_dir_recursive(&self.sftp, &path).await.map_err(sftp_error)
        } else {
            self.sftp.remove_file(&path).await.map_err(sftp_error)
        }
    }

    async fn home(&self) -> Result<PathBuf, ProtocolError> {
        Ok(PathBuf::from(self.sftp.canonicalize(".").await.map_err(sftp_error)?))
    }

    async fn open_read(&self, path: &Path, offset: u64) -> Result<Reader, ProtocolError> {
        let mut file = self.sftp.open(remote(path)).await.map_err(sftp_error)?;
        file.seek(SeekFrom::Start(offset)).await?;
        Ok(Box::new(file))
    }

    async fn open_write(&self, path: &Path, offset: u64) -> Result<Writer, ProtocolError> {
        let path = remote(path);
        if offset > 0
            && let Ok(mut file) = self.sftp.open_with_flags(&path, OpenFlags::WRITE | OpenFlags::CREATE).await
        {
            let mut size = FileAttributes::empty();
            size.size = Some(offset);
            let _ = file.set_metadata(size).await;
            file.seek(SeekFrom::Start(offset)).await?;
            return Ok(Writer { stream: Box::new(file), offset });
        }
        Ok(Writer { stream: Box::new(self.sftp.create(&path).await.map_err(sftp_error)?), offset: 0 })
    }

    fn resume_backoff(&self) -> u64 {
        SFTP_RESUME_BACKOFF_BYTES
    }

    async fn search(&self, query: SearchQuery, tx: SearchSender, cancel: Arc<AtomicBool>) {
        crate::search::search_remote(&self.session, &self.sftp, query, tx, cancel).await;
    }
}

#[cfg(test)]
mod tests;
