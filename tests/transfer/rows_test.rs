use std::path::PathBuf;

use super::*;

fn job(queue: &mut TransferQueue, name: &str, batch_id: Option<u64>, status: JobStatus) -> u64 {
    let id = queue.enqueue(1, Direction::Upload, PathBuf::from(format!("/local/{name}")), format!("/remote/{name}"), name.to_string(), 100, batch_id);
    let job = queue.get_mut(id).unwrap();
    if status == JobStatus::Completed {
        job.transferred_bytes = 100;
    }
    job.status = status;
    id
}

fn state_of(statuses: Vec<JobStatus>) -> RowState {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch("photos".to_string());
    for (index, status) in statuses.into_iter().enumerate() {
        job(&mut queue, &format!("{index}.txt"), Some(batch_id), status);
    }
    queue_rows(&queue, &[]).remove(0).state
}

#[test]
fn a_batch_is_one_row_and_singles_are_their_own_rows_in_queue_order() {
    let mut queue = TransferQueue::new();
    let single_before = job(&mut queue, "before.txt", None, JobStatus::Queued);
    let batch_id = queue.start_batch("photos".to_string());
    let first_in_batch = job(&mut queue, "a.jpg", Some(batch_id), JobStatus::Completed);
    let single_between = job(&mut queue, "between.txt", None, JobStatus::Queued);
    let second_in_batch = job(&mut queue, "b.jpg", Some(batch_id), JobStatus::Queued);

    let rows = queue_rows(&queue, &[]);

    assert_eq!(rows.iter().map(|row| row.kind).collect::<Vec<_>>(), vec![RowKind::Single(single_before), RowKind::Batch(batch_id), RowKind::Single(single_between)]);
    assert_eq!(rows[1].label, "photos");
    assert_eq!(rows[1].job_ids, vec![first_in_batch, second_in_batch]);
    assert_eq!((rows[1].files_done, rows[1].files_total, rows[1].bytes_done, rows[1].bytes_total), (1, 2, 100, 200));
    assert_eq!(rows[0].label, "before.txt");
}

#[test]
fn a_batch_without_a_stored_label_uses_its_first_files_name() {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch("photos".to_string());
    let only = job(&mut queue, "sub/a.jpg", Some(batch_id), JobStatus::Queued);
    let other = job(&mut queue, "x.txt", None, JobStatus::Completed);
    queue.remove_jobs(&[only]);
    let new_only = job(&mut queue, "sub/b.jpg", Some(batch_id), JobStatus::Queued);

    let rows = queue_rows(&queue, &[]);

    assert_eq!(rows.iter().find(|row| row.kind == RowKind::Batch(batch_id)).unwrap().label, "sub/b.jpg");
    assert!(rows.iter().any(|row| row.kind == RowKind::Single(other)));
    assert_eq!(rows.iter().find(|row| row.kind == RowKind::Batch(batch_id)).unwrap().job_ids, vec![new_only]);
}

#[test]
fn row_state_follows_the_precedence_rules() {
    let failed = || JobStatus::Failed("boom".to_string());
    assert_eq!(state_of(vec![JobStatus::InProgress, JobStatus::Queued, failed()]), RowState::Running);
    assert_eq!(state_of(vec![JobStatus::Queued, JobStatus::Completed]), RowState::Queued);
    assert_eq!(state_of(vec![JobStatus::Completed, JobStatus::Completed]), RowState::Done);
    assert_eq!(state_of(vec![JobStatus::Cancelled, JobStatus::Cancelled]), RowState::Cancelled);
    assert_eq!(state_of(vec![JobStatus::Completed, JobStatus::Cancelled]), RowState::Cancelled);
    assert_eq!(state_of(vec![JobStatus::Completed, failed(), failed(), JobStatus::Cancelled]), RowState::PartlyFailed(2));
    assert_eq!(state_of(vec![failed(), JobStatus::Cancelled]), RowState::Failed);
}

#[test]
fn scans_are_appended_as_scanning_rows() {
    let mut queue = TransferQueue::new();
    let single = job(&mut queue, "a.txt", None, JobStatus::Queued);
    let scans = [ScanInfo { batch_id: 7, label: "photos", direction: Direction::Download }];

    let rows = queue_rows(&queue, &scans);

    assert_eq!(rows[0].kind, RowKind::Single(single));
    assert_eq!(rows[1].kind, RowKind::Scan(7));
    assert_eq!(rows[1].label, "photos");
    assert_eq!(rows[1].direction, Direction::Download);
    assert_eq!(rows[1].state, RowState::Scanning);
    assert!(rows[1].job_ids.is_empty());
}

#[test]
fn finished_rows_are_done_partly_failed_failed_or_cancelled() {
    assert!(!QueueRow::finished_state(&RowState::Scanning));
    assert!(!QueueRow::finished_state(&RowState::Running));
    assert!(!QueueRow::finished_state(&RowState::Queued));
    for state in [RowState::Done, RowState::PartlyFailed(1), RowState::Failed, RowState::Cancelled] {
        assert!(QueueRow::finished_state(&state));
    }
}

#[test]
fn percent_of_handles_zero_totals() {
    assert_eq!(percent_of(0, 0), 100);
    assert_eq!(percent_of(25, 100), 25);
}
