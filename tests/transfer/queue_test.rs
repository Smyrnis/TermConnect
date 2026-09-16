use super::*;

fn queue_with_one_job(queue: &mut TransferQueue) -> u64 {
    queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/file.txt"),
        "/remote/file.txt".to_string(),
        "file.txt".to_string(),
        100,
    )
}

#[test]
fn enqueue_assigns_increasing_ids() {
    let mut queue = TransferQueue::new();
    let first = queue_with_one_job(&mut queue);
    let second = queue_with_one_job(&mut queue);
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
    let a1 = queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a1.txt"),
        "/remote/a1.txt".to_string(),
        "a1.txt".to_string(),
        10,
    );
    let a2 = queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a2.txt"),
        "/remote/a2.txt".to_string(),
        "a2.txt".to_string(),
        10,
    );
    let other = queue_with_one_job(&mut queue); // session_id 1 too, but...
    queue.get_mut(other).unwrap().session_id = 2;
    let in_progress = queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a3.txt"),
        "/remote/a3.txt".to_string(),
        "a3.txt".to_string(),
        10,
    );
    queue.get_mut(in_progress).unwrap().status = JobStatus::InProgress;

    let count = queue.fail_queued_for_session(1, "session disconnected");

    assert_eq!(count, 2);
    assert_eq!(
        queue.get(a1).unwrap().status,
        JobStatus::Failed("session disconnected".to_string())
    );
    assert_eq!(
        queue.get(a2).unwrap().status,
        JobStatus::Failed("session disconnected".to_string())
    );
    // Session 2's job is untouched.
    assert_eq!(queue.get(other).unwrap().status, JobStatus::Queued);
    // The in-progress job for session 1 is left for the caller to
    // cancel separately, not force-failed here.
    assert_eq!(
        queue.get(in_progress).unwrap().status,
        JobStatus::InProgress
    );
}
