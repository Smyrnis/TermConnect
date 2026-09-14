use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use russh_sftp::client::SftpSession;
use tokio::fs::File as LocalFile;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::Direction;

const CHUNK_SIZE: usize = 32 * 1024;
/// How often progress is reported back, in bytes — frequent enough to feel
/// live, infrequent enough not to flood the event channel on a fast link.
const PROGRESS_STEP_BYTES: u64 = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum TransferOutcome {
    Completed,
    Cancelled,
}

/// Runs one file transfer end to end, reporting cumulative bytes
/// transferred via `on_progress` every [`PROGRESS_STEP_BYTES`] and
/// checking `cancel` between chunks.
pub async fn execute(
    direction: Direction,
    local_path: &Path,
    remote_path: &str,
    sftp: &SftpSession,
    cancel: &AtomicBool,
    on_progress: impl FnMut(u64),
) -> Result<TransferOutcome> {
    match direction {
        Direction::Upload => {
            let mut source = LocalFile::open(local_path).await?;
            let mut destination = sftp.create(remote_path).await?;
            let outcome =
                copy_with_progress(&mut source, &mut destination, cancel, on_progress).await?;
            destination.shutdown().await?;
            Ok(outcome)
        }
        Direction::Download => {
            let mut source = sftp.open(remote_path).await?;
            let mut destination = LocalFile::create(local_path).await?;
            copy_with_progress(&mut source, &mut destination, cancel, on_progress).await
        }
    }
}

async fn copy_with_progress<R, W>(
    source: &mut R,
    destination: &mut W,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64),
) -> Result<TransferOutcome>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut transferred: u64 = 0;
    let mut since_last_report: u64 = 0;

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
