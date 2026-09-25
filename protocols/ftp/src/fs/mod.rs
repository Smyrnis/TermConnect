use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use futures_util::future::BoxFuture;
use porthmos_vfs::{
    DirItem, Entry, ErrorKind, FileKind, FileSystem, Metadata, ProtocolError, Reader, Writer, async_trait, join_remote,
    path_to_remote_string,
};
use suppaftp::{FtpError, FtpResult, Status};
use tokio::io::AsyncBufReadExt;

use crate::{
    errors::ftp_error,
    listing::{ListedItem, now_seconds, parse_list_line, parse_mlsd_line, parse_mlst_facts},
    pool::{Pool, is_connection_lost, is_not_implemented, timed_out, within},
    session::Connection,
    streams::{FtpReader, FtpWriter, WriteStart, fallback, first_write_start},
};

pub(crate) struct FtpFs {
    pool: Arc<Pool>,
    home: PathBuf,
}

fn not_found(path: &str) -> ProtocolError {
    ProtocolError::new(ErrorKind::NotFound, anyhow!("No such file or directory: {path}"))
}

fn is_unavailable(err: &FtpError) -> bool {
    matches!(err, FtpError::UnexpectedResponse(response) if response.status == Status::FileUnavailable)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SimpleCommand {
    MakeDir,
    RemoveFile,
    RemoveDir,
}

async fn fetch_lines(connection: &mut Connection, command: String, idle: Duration) -> FtpResult<Vec<String>> {
    let (_, mut stream) =
        within(idle, connection.custom_data_command(command, &[Status::AboutToSend, Status::AlreadyOpen])).await?;
    let mut lines = Vec::new();
    let mut reader = tokio::io::BufReader::new(&mut stream);
    loop {
        let mut line = Vec::new();
        match tokio::time::timeout(idle, reader.read_until(b'\n', &mut line)).await {
            Err(_) => return Err(timed_out()),
            Ok(Ok(0)) => break,
            Ok(Ok(_)) => {
                let text = String::from_utf8_lossy(&line);
                let text = text.trim_end_matches(['\r', '\n']);
                if !text.is_empty() {
                    lines.push(text.to_string());
                }
            }
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Ok(Err(err)) => return Err(FtpError::ConnectionError(err)),
        }
    }
    drop(reader);
    within(idle, stream.finish()).await?;
    Ok(lines)
}

fn already_done(command: SimpleCommand, exists_now: bool) -> bool {
    match command {
        SimpleCommand::MakeDir => exists_now,
        SimpleCommand::RemoveFile | SimpleCommand::RemoveDir => !exists_now,
    }
}

impl FtpFs {
    pub(crate) async fn new(pool: Pool) -> Result<Self, ProtocolError> {
        let home = PathBuf::from(pool.run(|connection| Box::pin(connection.pwd())).await?);
        Ok(Self { pool: Arc::new(pool), home })
    }

    async fn mlsd(&self, path: &str) -> Result<Option<Vec<ListedItem>>, ProtocolError> {
        let owned = path.to_string();
        let idle = self.pool.timeout();
        let result = self
            .pool
            .run_raw_listing(|connection| {
                let path = owned.clone();
                Box::pin(async move { fetch_lines(connection, format!("MLSD {path}"), idle).await })
            })
            .await?;
        match result {
            Ok(lines) => Ok(Some(lines.iter().filter_map(|line| parse_mlsd_line(line)).collect())),
            Err(err) if is_not_implemented(&err) => {
                self.pool.mark_mlst_missing();
                Ok(None)
            }
            Err(err) => Err(ftp_error(err)),
        }
    }

    async fn items(&self, dir: &Path) -> Result<Vec<ListedItem>, ProtocolError> {
        let path = path_to_remote_string(dir);
        if self.pool.supports_mlst()
            && let Some(items) = self.mlsd(&path).await?
        {
            return Ok(items);
        }
        let now = now_seconds();
        let idle = self.pool.timeout();
        let lines = self
            .pool
            .run_raw_listing(|connection| {
                let path = path.clone();
                Box::pin(async move { fetch_lines(connection, format!("LIST {path}"), idle).await })
            })
            .await?
            .map_err(ftp_error)?;
        Ok(lines.iter().filter_map(|line| parse_list_line(line, now)).collect())
    }

    async fn is_directory(&self, path: &str) -> Result<bool, ProtocolError> {
        let path = path.to_string();
        self.pool
            .run(|connection| {
                let path = path.clone();
                Box::pin(async move {
                    let previous = connection.pwd().await?;
                    let entered = connection.cwd(&path).await.is_ok();
                    if entered {
                        connection.cwd(&previous).await?;
                    }
                    Ok(entered)
                })
            })
            .await
    }

    async fn mlst(&self, path: &str) -> Result<Option<Metadata>, ProtocolError> {
        let owned = path.to_string();
        let result = self
            .pool
            .run_raw(|connection| {
                let path = owned.clone();
                Box::pin(async move { connection.mlst(Some(&path)).await })
            })
            .await?;
        match result {
            Ok(line) => parse_mlst_facts(&line).map(Some).ok_or_else(|| not_found(path)),
            Err(err) if is_not_implemented(&err) => {
                self.pool.mark_mlst_missing();
                Ok(None)
            }
            Err(err) if is_unavailable(&err) => Err(not_found(path)),
            Err(err) => Err(ftp_error(err)),
        }
    }

    async fn stat_remote(&self, path: &str) -> Result<Metadata, ProtocolError> {
        if self.pool.supports_mlst()
            && let Some(metadata) = self.mlst(path).await?
        {
            return Ok(metadata);
        }
        let owned = path.to_string();
        let size = self
            .pool
            .run_raw(|connection| {
                let path = owned.clone();
                Box::pin(async move { connection.size(&path).await })
            })
            .await?;
        if let Ok(size) = size {
            let modified = self
                .pool
                .run_raw(|connection| {
                    let path = owned.clone();
                    Box::pin(async move { connection.mdtm(&path).await })
                })
                .await?
                .ok()
                .and_then(|time| u64::try_from(time.and_utc().timestamp()).ok());
            return Ok(Metadata { size: size as u64, modified, kind: FileKind::File, permissions: None });
        }
        if self.is_directory(path).await? {
            return Ok(Metadata { size: 0, modified: None, kind: FileKind::Dir, permissions: None });
        }
        Err(not_found(path))
    }

    async fn exists(&self, path: &str) -> Result<bool, ProtocolError> {
        match self.stat_remote(path).await {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err),
        }
    }

    async fn simple_raw(&self, path: &str, command: SimpleCommand) -> Result<Result<(), FtpError>, ProtocolError> {
        let owned = path.to_string();
        let attempt = self
            .pool
            .attempt(self.pool.timeout(), |connection| {
                let path = owned.clone();
                Box::pin(async move {
                    match command {
                        SimpleCommand::MakeDir => connection.mkdir(&path).await,
                        SimpleCommand::RemoveFile => connection.rm(&path).await,
                        SimpleCommand::RemoveDir => connection.rmdir(&path).await,
                    }
                })
            })
            .await?;
        match attempt.result {
            Err(err) if attempt.replayed && is_unavailable(&err) => {
                if already_done(command, self.exists(path).await?) {
                    Ok(Ok(()))
                } else {
                    Ok(Err(err))
                }
            }
            result => Ok(result),
        }
    }

    async fn simple(&self, path: &str, command: SimpleCommand) -> Result<(), ProtocolError> {
        self.simple_raw(path, command).await?.map_err(ftp_error)
    }

    async fn rename_raw(&self, from: &str, to: &str) -> Result<Result<(), FtpError>, ProtocolError> {
        let (from_owned, to_owned) = (from.to_string(), to.to_string());
        let attempt = self
            .pool
            .attempt(self.pool.timeout(), |connection| {
                let (from, to) = (from_owned.clone(), to_owned.clone());
                Box::pin(async move { connection.rename(&from, &to).await })
            })
            .await?;
        match attempt.result {
            Err(_) if attempt.replayed && !self.exists(from).await? && self.exists(to).await? => Ok(Ok(())),
            result => Ok(result),
        }
    }

    async fn remove_path(&self, path: String) -> Result<(), ProtocolError> {
        let refused = match self.simple_raw(&path, SimpleCommand::RemoveFile).await? {
            Ok(()) => return Ok(()),
            Err(err) => err,
        };
        if is_connection_lost(&refused) || !self.is_directory(&path).await? {
            return Err(ftp_error(refused));
        }
        self.delete_tree(path).await
    }

    fn delete_tree<'a>(&'a self, path: String) -> BoxFuture<'a, Result<(), ProtocolError>> {
        Box::pin(async move {
            for child in self.read_dir(Path::new(&path)).await? {
                let child_path = path_to_remote_string(&child.path);
                match child.metadata.kind {
                    FileKind::File => self.simple(&child_path, SimpleCommand::RemoveFile).await?,
                    FileKind::Dir | FileKind::Symlink => self.remove_path(child_path).await?,
                }
            }
            self.simple(&path, SimpleCommand::RemoveDir).await
        })
    }
}

#[async_trait]
impl FileSystem for FtpFs {
    async fn list(&self, dir: &Path) -> Result<Vec<Entry>, ProtocolError> {
        let parent = path_to_remote_string(dir);
        let mut entries = Vec::new();
        for item in self.items(dir).await? {
            let path = join_remote(&parent, &item.name);
            let is_dir = match item.metadata.kind {
                FileKind::Dir => true,
                FileKind::Symlink => self.is_directory(&path).await?,
                FileKind::File => false,
            };
            entries.push(Entry {
                path: PathBuf::from(path),
                is_dir,
                size: item.metadata.size,
                permissions: item.metadata.permissions,
                name: item.name,
            });
        }
        Ok(entries)
    }

    async fn read_dir(&self, dir: &Path) -> Result<Vec<DirItem>, ProtocolError> {
        let parent = path_to_remote_string(dir);
        Ok(self
            .items(dir)
            .await?
            .into_iter()
            .map(|item| DirItem {
                path: PathBuf::from(join_remote(&parent, &item.name)),
                metadata: item.metadata,
                name: item.name,
            })
            .collect())
    }

    async fn stat(&self, path: &Path) -> Result<Metadata, ProtocolError> {
        self.stat_remote(&path_to_remote_string(path)).await
    }

    async fn create_dir(&self, path: &Path) -> Result<(), ProtocolError> {
        self.simple(&path_to_remote_string(path), SimpleCommand::MakeDir).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> Result<(), ProtocolError> {
        let (from, to) = (path_to_remote_string(from), path_to_remote_string(to));
        let refused = match self.rename_raw(&from, &to).await? {
            Ok(()) => return Ok(()),
            Err(err) => err,
        };
        let backup = format!("{to}.bak");
        if !self.exists(&from).await? || !self.exists(&to).await? || self.exists(&backup).await? {
            return Err(ftp_error(refused));
        }
        self.rename_raw(&to, &backup).await?.map_err(ftp_error)?;
        match self.rename_raw(&from, &to).await? {
            Ok(()) => {
                let _ = self.simple(&backup, SimpleCommand::RemoveFile).await;
                Ok(())
            }
            Err(err) => {
                let _ = self.rename_raw(&backup, &to).await;
                Err(ftp_error(err))
            }
        }
    }

    async fn remove_file(&self, path: &Path) -> Result<(), ProtocolError> {
        self.simple(&path_to_remote_string(path), SimpleCommand::RemoveFile).await
    }

    async fn delete(&self, path: &Path) -> Result<(), ProtocolError> {
        self.remove_path(path_to_remote_string(path)).await
    }

    async fn home(&self) -> Result<PathBuf, ProtocolError> {
        Ok(self.home.clone())
    }

    async fn open_read(&self, path: &Path, offset: u64) -> Result<Reader, ProtocolError> {
        let path = path_to_remote_string(path);
        let mut connection = self.pool.borrow().await?;
        if offset > 0 {
            connection.resume_transfer(offset as usize).await.map_err(ftp_error)?;
        }
        let stream = connection.retr_as_stream(&path).await.map_err(ftp_error)?;
        Ok(Box::new(FtpReader::new(stream, connection, self.pool.clone())))
    }

    async fn open_write(&self, path: &Path, offset: u64) -> Result<Writer, ProtocolError> {
        let path = path_to_remote_string(path);
        let mut connection = self.pool.borrow().await?;
        let mut start = first_write_start(offset);
        loop {
            let opened = match start {
                WriteStart::Store => connection.put_with_stream(&path).await,
                WriteStart::Append => connection.append_with_stream(&path).await,
                WriteStart::RestartStore => match connection.resume_transfer(offset as usize).await {
                    Ok(()) => connection.put_with_stream(&path).await,
                    Err(err) => Err(err),
                },
            };
            match (opened, fallback(start)) {
                (Ok(stream), _) => {
                    let stream = Box::new(FtpWriter::new(stream, connection, self.pool.clone()));
                    return Ok(Writer { stream, offset: start.offset(offset) });
                }
                (Err(err), _) if is_connection_lost(&err) => return Err(ftp_error(err)),
                (Err(_), Some(next)) => start = next,
                (Err(err), None) => return Err(ftp_error(err)),
            }
        }
    }
}

#[cfg(test)]
mod tests;
