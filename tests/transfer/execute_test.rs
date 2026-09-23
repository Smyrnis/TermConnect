use super::*;

#[test]
fn part_path_appends_dot_part_to_the_final_path() {
    let path = Path::new("/tmp/downloads/report.pdf");

    assert_eq!(part_path(path), PathBuf::from("/tmp/downloads/report.pdf.part"));
}

#[tokio::test]
async fn finalize_local_renames_the_part_file_to_final_on_completed() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);
    tokio::fs::write(&part, b"contents").await.unwrap();

    finalize_local(&Ok(TransferOutcome::Completed), &part, &final_path).await.unwrap();

    assert!(final_path.exists());
    assert!(!part.exists());
}

#[tokio::test]
async fn finalize_local_keeps_the_part_file_on_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);
    tokio::fs::write(&part, b"contents").await.unwrap();

    finalize_local(&Ok(TransferOutcome::Cancelled), &part, &final_path).await.unwrap();

    assert!(!final_path.exists());
    assert!(part.exists());
}

#[tokio::test]
async fn finalize_local_keeps_the_part_file_on_error() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);
    tokio::fs::write(&part, b"contents").await.unwrap();

    finalize_local(&Err(anyhow::anyhow!("boom")), &part, &final_path).await.unwrap();

    assert!(!final_path.exists());
    assert!(part.exists());
}

#[tokio::test]
async fn finalize_local_on_a_missing_part_file_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);

    finalize_local(&Ok(TransferOutcome::Cancelled), &part, &final_path).await.unwrap();
}

const MIB: u64 = 1024 * 1024;

#[test]
fn resume_offset_backs_off_from_a_valid_partial() {
    assert_eq!(resume_offset(10 * MIB, 20 * MIB, Some(1_000), Some(900)), 10 * MIB - RESUME_BACKOFF_BYTES);
}

#[test]
fn resume_offset_is_zero_for_a_partial_smaller_than_the_backoff() {
    assert_eq!(resume_offset(1_000, 20 * MIB, Some(1_000), Some(900)), 0);
}

#[test]
fn resume_offset_restarts_when_the_partial_is_larger_than_the_source() {
    assert_eq!(resume_offset(30 * MIB, 20 * MIB, Some(1_000), Some(900)), 0);
}

#[test]
fn resume_offset_restarts_when_the_source_changed_after_the_partial() {
    assert_eq!(resume_offset(10 * MIB, 20 * MIB, Some(1_000), Some(1_000 + CLOCK_SKEW_TOLERANCE_SECS + 1)), 0);
}

#[test]
fn resume_offset_allows_small_clock_skew() {
    assert_eq!(
        resume_offset(10 * MIB, 20 * MIB, Some(1_000), Some(1_000 + CLOCK_SKEW_TOLERANCE_SECS)),
        10 * MIB - RESUME_BACKOFF_BYTES
    );
}

#[test]
fn resume_offset_restarts_without_modification_times() {
    assert_eq!(resume_offset(10 * MIB, 20 * MIB, None, Some(900)), 0);
    assert_eq!(resume_offset(10 * MIB, 20 * MIB, Some(1_000), None), 0);
}

#[tokio::test]
async fn open_local_part_trims_to_the_offset_and_appends_after_it() {
    use tokio::io::AsyncWriteExt;

    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.bin.part");
    tokio::fs::write(&part, b"0123456789").await.unwrap();

    let mut file = open_local_part(&part, 4).await.unwrap();
    file.write_all(b"XY").await.unwrap();
    file.flush().await.unwrap();

    assert_eq!(tokio::fs::read(&part).await.unwrap(), b"0123XY");
}

#[tokio::test]
async fn open_local_part_at_zero_truncates() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.bin.part");
    tokio::fs::write(&part, b"stale").await.unwrap();

    drop(open_local_part(&part, 0).await.unwrap());

    assert!(tokio::fs::read(&part).await.unwrap().is_empty());
}

#[tokio::test]
async fn copy_with_progress_reports_from_the_start_offset() {
    let source: &[u8] = b"abc";
    let mut destination: Vec<u8> = Vec::new();
    let mut reported = Vec::new();

    copy_with_progress(&mut &source[..], &mut destination, 100, &AtomicBool::new(false), |transferred| {
        reported.push(transferred)
    })
    .await
    .unwrap();

    assert_eq!(reported.first(), Some(&100));
    assert_eq!(reported.last(), Some(&103));
}

#[tokio::test]
async fn resuming_from_a_partial_with_a_bad_tail_reproduces_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let source: Vec<u8> = (0..(2 * RESUME_BACKOFF_BYTES + 777)).map(|index| (index % 251) as u8).collect();
    let good = RESUME_BACKOFF_BYTES as usize + 500;
    let mut partial = source[..good].to_vec();
    partial.extend(std::iter::repeat_n(0u8, 300));
    let part = dir.path().join("big.bin.part");
    tokio::fs::write(&part, &partial).await.unwrap();

    let offset = resume_offset(partial.len() as u64, source.len() as u64, Some(1_000), Some(900));
    let mut destination = open_local_part(&part, offset).await.unwrap();
    copy_with_progress(&mut &source[offset as usize..], &mut destination, offset, &AtomicBool::new(false), |_| {})
        .await
        .unwrap();
    drop(destination);

    assert!(offset > 0);
    assert_eq!(tokio::fs::read(&part).await.unwrap(), source);
}

#[test]
fn the_backoff_covers_every_pipelined_write() {
    use crate::connection::client::{SFTP_MAX_CONCURRENT_WRITES, SFTP_MAX_WRITE_PACKET_LEN};

    assert_eq!(RESUME_BACKOFF_BYTES, SFTP_MAX_CONCURRENT_WRITES as u64 * SFTP_MAX_WRITE_PACKET_LEN as u64);
}
