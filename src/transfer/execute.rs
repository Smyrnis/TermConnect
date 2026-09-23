use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use russh_sftp::{
    client::{SftpSession, fs::File as RemoteFile},
    protocol::{FileAttributes, OpenFlags},
};
use tokio::{
    fs::{File as LocalFile, OpenOptions},
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt},
};

use super::{Direction, plan::unix_seconds};
use crate::connection::client::{SFTP_MAX_CONCURRENT_WRITES, SFTP_MAX_WRITE_PACKET_LEN};

const CHUNK_SIZE: usize = 256 * 1024;
const PROGRESS_STEP_BYTES: u64 = 64 * 1024;
const RESUME_BACKOFF_BYTES: u64 = SFTP_MAX_CONCURRENT_WRITES as u64 * SFTP_MAX_WRITE_PACKET_LEN as u64;
const CLOCK_SKEW_TOLERANCE_SECS: u64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub enum TransferOutcome {
    Completed,
    Cancelled,
}

pub async fn execute(
    direction: Direction, local_path: &Path, remote_path: &str, sftp: &SftpSession, cancel: &AtomicBool, resume: bool,
    on_progress: impl FnMut(u64),
) -> Result<TransferOutcome> {
    match direction {
        Direction::Upload => {
            let remote_part_path = format!("{remote_path}.part");
            let local_metadata = tokio::fs::metadata(local_path).await?;
            let offset = if resume {
                remote_resume_offset(
                    sftp,
                    &remote_part_path,
                    local_metadata.len(),
                    unix_seconds(local_metadata.modified().ok()),
                )
                .await
            } else {
                0
            };
            let (mut destination, offset) = open_remote_part(sftp, &remote_part_path, offset).await?;
            let mut source = LocalFile::open(local_path).await?;
            source.seek(SeekFrom::Start(offset)).await?;
            let result = copy_with_progress(&mut source, &mut destination, offset, cancel, on_progress).await;
            let _ = destination.shutdown().await;
            finalize_remote(&result, sftp, &remote_part_path, remote_path).await?;
            result
        }
        Direction::Download => {
            let part = part_path(local_path);
            let offset = if resume { local_resume_offset(sftp, &part, remote_path).await } else { 0 };
            let mut source = sftp.open(remote_path).await?;
            source.seek(SeekFrom::Start(offset)).await?;
            let mut destination = open_local_part(&part, offset).await?;
            let result = copy_with_progress(&mut source, &mut destination, offset, cancel, on_progress).await;
            finalize_local(&result, &part, local_path).await?;
            result
        }
    }
}

pub(crate) fn resume_offset(
    part_len: u64, source_len: u64, part_modified: Option<u64>, source_modified: Option<u64>,
) -> u64 {
    let (Some(part_modified), Some(source_modified)) = (part_modified, source_modified) else {
        return 0;
    };
    if part_len > source_len || source_modified > part_modified + CLOCK_SKEW_TOLERANCE_SECS {
        return 0;
    }
    part_len.saturating_sub(RESUME_BACKOFF_BYTES)
}

pub(crate) async fn open_local_part(path: &Path, offset: u64) -> Result<LocalFile> {
    let mut file = OpenOptions::new().create(true).write(true).truncate(false).open(path).await?;
    file.set_len(offset).await?;
    file.seek(SeekFrom::Start(offset)).await?;
    Ok(file)
}

async fn open_remote_part(sftp: &SftpSession, path: &str, offset: u64) -> Result<(RemoteFile, u64)> {
    if offset > 0
        && let Ok(mut file) = sftp.open_with_flags(path, OpenFlags::WRITE | OpenFlags::CREATE).await
    {
        let mut size = FileAttributes::empty();
        size.size = Some(offset);
        let _ = file.set_metadata(size).await;
        file.seek(SeekFrom::Start(offset)).await?;
        return Ok((file, offset));
    }
    Ok((sftp.create(path).await?, 0))
}

async fn remote_resume_offset(
    sftp: &SftpSession, part_path: &str, source_len: u64, source_modified: Option<u64>,
) -> u64 {
    match sftp.metadata(part_path).await {
        Ok(part) => resume_offset(part.len(), source_len, part.mtime.map(u64::from), source_modified),
        Err(_) => 0,
    }
}

async fn local_resume_offset(sftp: &SftpSession, part_path: &Path, remote_path: &str) -> u64 {
    let Ok(part) = tokio::fs::metadata(part_path).await else {
        return 0;
    };
    let Ok(source) = sftp.metadata(remote_path).await else {
        return 0;
    };
    resume_offset(part.len(), source.len(), unix_seconds(part.modified().ok()), source.mtime.map(u64::from))
}

async fn copy_with_progress<R, W>(
    source: &mut R, destination: &mut W, start: u64, cancel: &AtomicBool, mut on_progress: impl FnMut(u64),
) -> Result<TransferOutcome>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut transferred: u64 = start;
    let mut since_last_report: u64 = 0;
    on_progress(start);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(TransferOutcome::Cancelled);
        }

        let bytes_read = source.read(&mut buf).await?;
        if bytes_read == 0 {
            break;
        }

        destination.write_all(&buf[..bytes_read]).await?;
        transferred += bytes_read as u64;
        since_last_report += bytes_read as u64;

        if since_last_report >= PROGRESS_STEP_BYTES {
            on_progress(transferred);
            since_last_report = 0;
        }
    }

    destination.flush().await?;
    on_progress(transferred);
    Ok(TransferOutcome::Completed)
}

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

async fn finalize_local(result: &Result<TransferOutcome>, part_path: &Path, final_path: &Path) -> Result<()> {
    if let Ok(TransferOutcome::Completed) = result {
        tokio::fs::rename(part_path, final_path).await?;
    }
    Ok(())
}

async fn finalize_remote(
    result: &Result<TransferOutcome>, sftp: &SftpSession, part_path: &str, final_path: &str,
) -> Result<()> {
    if let Ok(TransferOutcome::Completed) = result {
        crate::filesystem::remote::rename_overwriting(sftp, part_path, final_path).await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/transfer/execute_test.rs"]
mod tests;
