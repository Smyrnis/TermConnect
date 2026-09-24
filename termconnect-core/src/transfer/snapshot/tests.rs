use std::path::PathBuf;

use super::*;
use crate::transfer::{Direction, JobStatus, rows::RowState};

fn queue_with_jobs() -> (TransferQueue, u64, u64) {
    let mut queue = TransferQueue::new();
    let batch = queue.start_batch("photos".to_string());
    let running =
        queue.enqueue(1, Direction::Upload, PathBuf::from("/l/a"), "/r/a".into(), "a".into(), 100, Some(batch));
    queue.get_mut(running).unwrap().status = JobStatus::InProgress;
    queue.get_mut(running).unwrap().transferred_bytes = 50;
    queue.enqueue(1, Direction::Upload, PathBuf::from("/l/b"), "/r/b".into(), "b".into(), 100, Some(batch));
    (queue, batch, running)
}

#[test]
fn a_snapshot_lists_active_jobs_with_their_batch_progress() {
    let (queue, batch, running) = queue_with_jobs();

    let snapshot = TransferSnapshot::of(&queue, &[]);

    assert_eq!(snapshot.queued, 1);
    assert_eq!(snapshot.active.len(), 1);
    assert_eq!(snapshot.active[0].id, running);
    assert_eq!(snapshot.active[0].percent, 50);
    assert_eq!(snapshot.batches[&batch].total_files, 2);
    assert_eq!(snapshot.rows.len(), 1);
}

#[test]
fn scans_and_waiting_copies_are_counted_separately() {
    let queue = TransferQueue::new();
    let scans = [
        ScanInfo { batch_id: 1, label: "one", direction: Direction::Upload, state: RowState::Scanning },
        ScanInfo { batch_id: 2, label: "two", direction: Direction::Upload, state: RowState::AwaitingAnswer },
    ];

    let snapshot = TransferSnapshot::of(&queue, &scans);

    assert_eq!(snapshot.scanning, vec!["one".to_string()]);
    assert_eq!(snapshot.awaiting_answers, 1);
    assert_eq!(snapshot.rows.len(), 2);
}
