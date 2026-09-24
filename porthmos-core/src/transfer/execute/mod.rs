use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use porthmos_vfs::{FileSystem, ProtocolError};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const CHUNK_SIZE: usize = 256 * 1024;
const PROGRESS_STEP_BYTES: u64 = 64 * 1024;
const CLOCK_SKEW_TOLERANCE_SECS: u64 = 120;

#[derive(Debug, PartialEq, Eq)]
pub enum TransferOutcome {
    Completed,
    Cancelled,
}

pub async fn execute(
    source: &dyn FileSystem, source_path: &Path, destination: &dyn FileSystem, destination_path: &Path,
    cancel: &AtomicBool, resume: bool, on_progress: impl FnMut(u64) + Send,
) -> Result<TransferOutcome, ProtocolError> {
    let source_metadata = source.stat(source_path).await?;
    let part = part_path(destination_path);
    let offset = if resume {
        let backoff = source.resume_backoff().max(destination.resume_backoff());
        match destination.stat(&part).await {
            Ok(part_metadata) => resume_offset(
                part_metadata.size,
                source_metadata.size,
                part_metadata.modified,
                source_metadata.modified,
                backoff,
            ),
            Err(_) => 0,
        }
    } else {
        0
    };
    let mut writer = destination.open_write(&part, offset).await?;
    let mut reader = source.open_read(source_path, writer.offset).await?;
    let result = copy_with_progress(&mut reader, &mut writer.stream, writer.offset, cancel, on_progress).await;
    let _ = writer.stream.shutdown().await;
    if let Ok(TransferOutcome::Completed) = result {
        destination.rename(&part, destination_path).await?;
    }
    result
}

pub(crate) fn resume_offset(
    part_len: u64, source_len: u64, part_modified: Option<u64>, source_modified: Option<u64>, backoff: u64,
) -> u64 {
    let (Some(part_modified), Some(source_modified)) = (part_modified, source_modified) else {
        return 0;
    };
    if part_len > source_len || source_modified > part_modified + CLOCK_SKEW_TOLERANCE_SECS {
        return 0;
    }
    part_len.saturating_sub(backoff)
}

async fn copy_with_progress<R, W>(
    source: &mut R, destination: &mut W, start: u64, cancel: &AtomicBool, mut on_progress: impl FnMut(u64),
) -> Result<TransferOutcome, ProtocolError>
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

pub fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests;
