use porthmos_vfs::testing::FakeFs;

use super::*;

const SFTP_LIKE_BACKOFF: u64 = 16 * 32 * 1024;

#[test]
fn part_path_appends_dot_part_to_the_final_path() {
    let path = Path::new("/tmp/downloads/report.pdf");

    assert_eq!(part_path(path), PathBuf::from("/tmp/downloads/report.pdf.part"));
}

fn source_with(data: &[u8]) -> FakeFs {
    let source = FakeFs::new();
    source.file("/src/f", data, Some(900));
    source
}

fn empty_destination() -> FakeFs {
    let destination = FakeFs::new();
    destination.dir("/dst");
    destination
}

#[tokio::test]
async fn a_completed_copy_replaces_the_part_file_with_the_final_file() {
    let source = source_with(b"payload");
    let destination = empty_destination();

    let outcome = execute(
        &source,
        Path::new("/src/f"),
        &destination,
        Path::new("/dst/f"),
        &AtomicBool::new(false),
        false,
        |_| {},
    )
    .await
    .unwrap();

    assert_eq!(outcome, TransferOutcome::Completed);
    assert_eq!(destination.contents("/dst/f").unwrap(), b"payload");
    assert!(!destination.exists("/dst/f.part"));
}

#[tokio::test]
async fn a_cancelled_copy_keeps_the_part_file() {
    let source = source_with(b"payload");
    let destination = empty_destination();

    let outcome =
        execute(&source, Path::new("/src/f"), &destination, Path::new("/dst/f"), &AtomicBool::new(true), false, |_| {})
            .await
            .unwrap();

    assert_eq!(outcome, TransferOutcome::Cancelled);
    assert!(destination.exists("/dst/f.part"));
    assert!(!destination.exists("/dst/f"));
}

#[tokio::test]
async fn a_failed_read_keeps_the_part_file_and_reports_the_error() {
    let source = source_with(b"payload");
    source.fail_reads("/src/f");
    let destination = empty_destination();

    let result = execute(
        &source,
        Path::new("/src/f"),
        &destination,
        Path::new("/dst/f"),
        &AtomicBool::new(false),
        false,
        |_| {},
    )
    .await;

    assert!(result.is_err());
    assert!(destination.exists("/dst/f.part"));
    assert!(!destination.exists("/dst/f"));
}

#[tokio::test]
async fn a_missing_source_fails_before_any_part_file_is_created() {
    let source = FakeFs::new();
    let destination = empty_destination();

    let result = execute(
        &source,
        Path::new("/src/gone"),
        &destination,
        Path::new("/dst/gone"),
        &AtomicBool::new(false),
        false,
        |_| {},
    )
    .await;

    assert!(result.is_err());
    assert!(!destination.exists("/dst/gone.part"));
}

#[tokio::test]
async fn resume_backs_off_by_the_larger_of_source_and_destination() {
    let source = FakeFs::new().with_resume_backoff(0);
    source.file("/src/f", &[7u8; 20], Some(1));
    let destination = FakeFs::new().with_resume_backoff(4);
    destination.file("/dst/f.part", &[7u8; 10], Some(2));
    let mut reported = Vec::new();

    execute(&source, Path::new("/src/f"), &destination, Path::new("/dst/f"), &AtomicBool::new(false), true, |bytes| {
        reported.push(bytes)
    })
    .await
    .unwrap();

    assert_eq!(reported[0], 6);
    assert_eq!(destination.contents("/dst/f").unwrap(), vec![7u8; 20]);
}

#[tokio::test]
async fn resume_uses_the_source_backoff_when_it_is_the_larger_one() {
    let source = FakeFs::new().with_resume_backoff(8);
    source.file("/src/f", &[1u8; 20], Some(1));
    let destination = FakeFs::new();
    destination.file("/dst/f.part", &[1u8; 10], Some(2));
    let mut reported = Vec::new();

    execute(&source, Path::new("/src/f"), &destination, Path::new("/dst/f"), &AtomicBool::new(false), true, |bytes| {
        reported.push(bytes)
    })
    .await
    .unwrap();

    assert_eq!(reported[0], 2);
}

const MIB: u64 = 1024 * 1024;

#[test]
fn resume_offset_backs_off_from_a_valid_partial() {
    assert_eq!(
        resume_offset(10 * MIB, 20 * MIB, Some(1_000), Some(900), SFTP_LIKE_BACKOFF),
        10 * MIB - SFTP_LIKE_BACKOFF
    );
}

#[test]
fn resume_offset_is_zero_for_a_partial_smaller_than_the_backoff() {
    assert_eq!(resume_offset(1_000, 20 * MIB, Some(1_000), Some(900), SFTP_LIKE_BACKOFF), 0);
}

#[test]
fn resume_offset_restarts_when_the_partial_is_larger_than_the_source() {
    assert_eq!(resume_offset(30 * MIB, 20 * MIB, Some(1_000), Some(900), SFTP_LIKE_BACKOFF), 0);
}

#[test]
fn resume_offset_restarts_when_the_source_changed_after_the_partial() {
    assert_eq!(
        resume_offset(10 * MIB, 20 * MIB, Some(1_000), Some(1_000 + CLOCK_SKEW_TOLERANCE_SECS + 1), SFTP_LIKE_BACKOFF),
        0
    );
}

#[test]
fn resume_offset_allows_small_clock_skew() {
    assert_eq!(
        resume_offset(10 * MIB, 20 * MIB, Some(1_000), Some(1_000 + CLOCK_SKEW_TOLERANCE_SECS), SFTP_LIKE_BACKOFF),
        10 * MIB - SFTP_LIKE_BACKOFF
    );
}

#[test]
fn resume_offset_restarts_without_modification_times() {
    assert_eq!(resume_offset(10 * MIB, 20 * MIB, None, Some(900), SFTP_LIKE_BACKOFF), 0);
    assert_eq!(resume_offset(10 * MIB, 20 * MIB, Some(1_000), None, SFTP_LIKE_BACKOFF), 0);
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
    let content: Vec<u8> = (0..(2 * SFTP_LIKE_BACKOFF + 777)).map(|index| (index % 251) as u8).collect();
    let good = SFTP_LIKE_BACKOFF as usize + 500;
    let mut partial = content[..good].to_vec();
    partial.extend(std::iter::repeat_n(0u8, 300));
    let source = FakeFs::new();
    source.file("/src/big.bin", &content, Some(900));
    let destination = FakeFs::new().with_resume_backoff(SFTP_LIKE_BACKOFF);
    destination.file("/dst/big.bin.part", &partial, Some(1_000));
    let mut reported = Vec::new();

    execute(
        &source,
        Path::new("/src/big.bin"),
        &destination,
        Path::new("/dst/big.bin"),
        &AtomicBool::new(false),
        true,
        |bytes| reported.push(bytes),
    )
    .await
    .unwrap();

    assert!(reported[0] > 0);
    assert_eq!(destination.contents("/dst/big.bin").unwrap(), content);
}

#[tokio::test]
async fn an_upload_whose_final_shutdown_fails_is_reported_and_not_renamed() {
    let source = source_with(b"payload");
    let destination = empty_destination();
    destination.fail_shutdown("/dst/f.part");

    let result = execute(
        &source,
        Path::new("/src/f"),
        &destination,
        Path::new("/dst/f"),
        &AtomicBool::new(false),
        false,
        |_| {},
    )
    .await;

    assert!(result.is_err());
    assert!(!destination.exists("/dst/f"));
    assert!(destination.exists("/dst/f.part"));
}

#[tokio::test]
async fn the_destination_learns_the_size_of_the_transfer() {
    let source = source_with(b"payload");
    let destination = empty_destination();

    execute(&source, Path::new("/src/f"), &destination, Path::new("/dst/f"), &AtomicBool::new(false), false, |_| {})
        .await
        .unwrap();

    assert_eq!(destination.written_sizes(), vec![(PathBuf::from("/dst/f.part"), 7)]);
}
