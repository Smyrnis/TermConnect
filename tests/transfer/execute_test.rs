use super::*;

#[test]
fn part_path_appends_dot_part_to_the_final_path() {
    let path = Path::new("/tmp/downloads/report.pdf");

    assert_eq!(
        part_path(path),
        PathBuf::from("/tmp/downloads/report.pdf.part")
    );
}

#[tokio::test]
async fn finalize_local_renames_the_part_file_to_final_on_completed() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);
    tokio::fs::write(&part, b"contents").await.unwrap();

    finalize_local(&Ok(TransferOutcome::Completed), &part, &final_path)
        .await
        .unwrap();

    assert!(final_path.exists());
    assert!(!part.exists());
}

#[tokio::test]
async fn finalize_local_removes_the_part_file_on_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);
    tokio::fs::write(&part, b"contents").await.unwrap();

    finalize_local(&Ok(TransferOutcome::Cancelled), &part, &final_path)
        .await
        .unwrap();

    assert!(!final_path.exists());
    assert!(!part.exists());
}

#[tokio::test]
async fn finalize_local_removes_the_part_file_on_error() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);
    tokio::fs::write(&part, b"contents").await.unwrap();

    finalize_local(&Err(anyhow::anyhow!("boom")), &part, &final_path)
        .await
        .unwrap();

    assert!(!final_path.exists());
    assert!(!part.exists());
}

#[tokio::test]
async fn finalize_local_on_a_missing_part_file_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("report.pdf");
    let part = part_path(&final_path);

    finalize_local(&Ok(TransferOutcome::Cancelled), &part, &final_path)
        .await
        .unwrap();
}
