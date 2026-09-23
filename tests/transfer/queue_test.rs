use super::*;

fn queue_with_one_job(queue: &mut TransferQueue) -> u64 {
    queue.enqueue(1, Direction::Upload, PathBuf::from("/local/file.txt"), "/remote/file.txt".to_string(), "file.txt".to_string(), 100, None)
}

fn queue_with_a_batch_job(queue: &mut TransferQueue, batch_id: u64, name: &str, total_bytes: u64) -> u64 {
    queue.enqueue(1, Direction::Upload, PathBuf::from(format!("/local/{name}")), format!("/remote/{name}"), name.to_string(), total_bytes, Some(batch_id))
}

#[test]
fn enqueue_assigns_increasing_ids() {
    let mut queue = TransferQueue::new();
    let first = queue_with_one_job(&mut queue);
    let second = queue_with_one_job(&mut queue);
    assert_ne!(first, second);
}

#[test]
fn start_batch_assigns_increasing_ids() {
    let mut queue = TransferQueue::new();
    let first = queue.start_batch();
    let second = queue.start_batch();
    assert_ne!(first, second);
}

#[test]
fn next_to_run_returns_the_first_queued_job() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    assert_eq!(queue.next_to_run(), Some(id));
}

#[test]
fn next_to_run_is_none_while_a_job_is_active() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    queue.get_mut(id).unwrap().status = JobStatus::InProgress;
    queue_with_one_job(&mut queue);

    assert_eq!(queue.next_to_run(), None);
}

#[test]
fn retry_or_give_up_requeues_within_the_attempt_limit() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    let job = queue.get_mut(id).unwrap();
    job.status = JobStatus::Failed("boom".to_string());
    job.attempts = 1;
    job.transferred_bytes = 50;

    assert!(queue.retry_or_give_up(id));
    let job = queue.get(id).unwrap();
    assert_eq!(job.status, JobStatus::Queued);
    assert_eq!(job.transferred_bytes, 0);
}

#[test]
fn retry_or_give_up_stops_after_max_attempts() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    let job = queue.get_mut(id).unwrap();
    job.status = JobStatus::Failed("boom".to_string());
    job.attempts = 3;

    assert!(!queue.retry_or_give_up(id));
}

#[test]
fn fail_queued_for_session_marks_only_that_sessions_queued_jobs() {
    let mut queue = TransferQueue::new();
    let a1 = queue.enqueue(1, Direction::Upload, PathBuf::from("/local/a1.txt"), "/remote/a1.txt".to_string(), "a1.txt".to_string(), 10, None);
    let a2 = queue.enqueue(1, Direction::Upload, PathBuf::from("/local/a2.txt"), "/remote/a2.txt".to_string(), "a2.txt".to_string(), 10, None);
    let other = queue_with_one_job(&mut queue);
    queue.get_mut(other).unwrap().session_id = 2;
    let in_progress = queue.enqueue(1, Direction::Upload, PathBuf::from("/local/a3.txt"), "/remote/a3.txt".to_string(), "a3.txt".to_string(), 10, None);
    queue.get_mut(in_progress).unwrap().status = JobStatus::InProgress;

    let count = queue.fail_queued_for_session(1, "session disconnected");

    assert_eq!(count, 2);
    assert_eq!(queue.get(a1).unwrap().status, JobStatus::Failed("session disconnected".to_string()));
    assert_eq!(queue.get(a2).unwrap().status, JobStatus::Failed("session disconnected".to_string()));
    assert_eq!(queue.get(other).unwrap().status, JobStatus::Queued);
    assert_eq!(queue.get(in_progress).unwrap().status, JobStatus::InProgress);
}

#[test]
fn batch_progress_aggregates_across_every_job_in_the_batch() {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch();
    let first = queue_with_a_batch_job(&mut queue, batch_id, "a.txt", 100);
    let second = queue_with_a_batch_job(&mut queue, batch_id, "b.txt", 200);
    let other_batch = queue.start_batch();
    queue_with_a_batch_job(&mut queue, other_batch, "c.txt", 999);

    queue.get_mut(first).unwrap().status = JobStatus::Completed;
    queue.get_mut(first).unwrap().transferred_bytes = 100;
    queue.get_mut(second).unwrap().status = JobStatus::InProgress;
    queue.get_mut(second).unwrap().transferred_bytes = 50;

    let progress = queue.batch_progress(batch_id);

    assert_eq!(progress.total_files, 2);
    assert_eq!(progress.completed_files, 1);
    assert_eq!(progress.total_bytes, 300);
    assert_eq!(progress.transferred_bytes, 150);
}

#[test]
fn cancel_batch_marks_only_queued_jobs_in_that_batch() {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch();
    let in_progress = queue_with_a_batch_job(&mut queue, batch_id, "a.txt", 100);
    let queued = queue_with_a_batch_job(&mut queue, batch_id, "b.txt", 100);
    let other_batch = queue.start_batch();
    let other = queue_with_a_batch_job(&mut queue, other_batch, "c.txt", 100);
    queue.get_mut(in_progress).unwrap().status = JobStatus::InProgress;

    let count = queue.cancel_batch(batch_id);

    assert_eq!(count, 1);
    assert_eq!(queue.get(in_progress).unwrap().status, JobStatus::InProgress);
    assert_eq!(queue.get(queued).unwrap().status, JobStatus::Cancelled);
    assert_eq!(queue.get(other).unwrap().status, JobStatus::Queued);
}
