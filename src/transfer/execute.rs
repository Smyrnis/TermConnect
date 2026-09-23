use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use russh_sftp::client::SftpSession;
use tokio::{
    fs::File as LocalFile,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
};

use super::Direction;

const CHUNK_SIZE: usize = 256 * 1024;
const PROGRESS_STEP_BYTES: u64 = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum TransferOutcome {
    Completed,
    Cancelled,
}

pub async fn execute(direction: Direction, local_path: &Path, remote_path: &str, sftp: &SftpSession, cancel: &AtomicBool, on_progress: impl FnMut(u64)) -> Result<TransferOutcome> {
    match direction {
        Direction::Upload => {
            let remote_part_path = format!("{remote_path}.part");
            let mut source = LocalFile::open(local_path).await?;
            let mut destination = sftp.create(&remote_part_path).await?;
            let result = copy_with_progress(&mut source, &mut destination, cancel, on_progress).await;
            let _ = destination.shutdown().await;
            finalize_remote(&result, sftp, &remote_part_path, remote_path).await?;
            result
        }
        Direction::Download => {
            let part = part_path(local_path);
            let mut source = sftp.open(remote_path).await?;
            let mut destination = LocalFile::create(&part).await?;
            let result = copy_with_progress(&mut source, &mut destination, cancel, on_progress).await;
            finalize_local(&result, &part, local_path).await?;
            result
        }
    }
}

async fn copy_with_progress<R, W>(source: &mut R, destination: &mut W, cancel: &AtomicBool, mut on_progress: impl FnMut(u64)) -> Result<TransferOutcome>
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

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

async fn finalize_local(result: &Result<TransferOutcome>, part_path: &Path, final_path: &Path) -> Result<()> {
    match result {
        Ok(TransferOutcome::Completed) => {
            tokio::fs::rename(part_path, final_path).await?;
        }
        _ => match tokio::fs::remove_file(part_path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        },
    }
    Ok(())
}

async fn finalize_remote(result: &Result<TransferOutcome>, sftp: &SftpSession, part_path: &str, final_path: &str) -> Result<()> {
    match result {
        Ok(TransferOutcome::Completed) => {
            crate::filesystem::remote::rename_overwriting(sftp, part_path, final_path).await?;
        }
        _ => {
            let _ = sftp.remove_file(part_path).await;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/transfer/execute_test.rs"]
mod tests;
