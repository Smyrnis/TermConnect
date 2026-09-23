use super::*;

fn queue_with_one_job(queue: &mut TransferQueue) -> u64 {
    queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/file.txt"),
        "/remote/file.txt".to_string(),
        "file.txt".to_string(),
        100,
        None,
    )
}

fn queue_with_a_batch_job(queue: &mut TransferQueue, batch_id: u64, name: &str, total_bytes: u64) -> u64 {
    queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        total_bytes,
        Some(batch_id),
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
fn start_batch_assigns_increasing_ids() {
    let mut queue = TransferQueue::new();
    let first = queue.start_batch("batch".to_string());
    let second = queue.start_batch("batch".to_string());
    assert_ne!(first, second);
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
    assert_eq!(job.transferred_bytes, 50);
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
        None,
    );
    let a2 = queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a2.txt"),
        "/remote/a2.txt".to_string(),
        "a2.txt".to_string(),
        10,
        None,
    );
    let other = queue_with_one_job(&mut queue);
    queue.get_mut(other).unwrap().session_id = 2;
    let in_progress = queue.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a3.txt"),
        "/remote/a3.txt".to_string(),
        "a3.txt".to_string(),
        10,
        None,
    );
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
    let batch_id = queue.start_batch("batch".to_string());
    let first = queue_with_a_batch_job(&mut queue, batch_id, "a.txt", 100);
    let second = queue_with_a_batch_job(&mut queue, batch_id, "b.txt", 200);
    let other_batch = queue.start_batch("batch".to_string());
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

fn enqueue_for(queue: &mut TransferQueue, session_id: u64, direction: Direction, name: &str) -> u64 {
    queue.enqueue(
        session_id,
        direction,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        10,
        None,
    )
}

#[test]
fn startable_returns_queued_jobs_in_order_up_to_the_limit() {
    let mut queue = TransferQueue::new();
    let first = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    let second = enqueue_for(&mut queue, 1, Direction::Upload, "b.txt");
    enqueue_for(&mut queue, 1, Direction::Upload, "c.txt");

    assert_eq!(queue.startable(2), vec![first, second]);
}

#[test]
fn startable_counts_jobs_that_are_already_active() {
    let mut queue = TransferQueue::new();
    let active = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    queue.get_mut(active).unwrap().status = JobStatus::InProgress;
    let next = enqueue_for(&mut queue, 1, Direction::Upload, "b.txt");
    enqueue_for(&mut queue, 1, Direction::Upload, "c.txt");

    assert_eq!(queue.startable(2), vec![next]);
}

#[test]
fn startable_with_a_limit_of_one_waits_for_the_active_job() {
    let mut queue = TransferQueue::new();
    let active = queue_with_one_job(&mut queue);
    queue.get_mut(active).unwrap().status = JobStatus::InProgress;
    queue_with_one_job(&mut queue);

    assert!(queue.startable(1).is_empty());
}

#[test]
fn startable_skips_finished_failed_and_cancelled_jobs() {
    let mut queue = TransferQueue::new();
    for status in [JobStatus::Completed, JobStatus::Failed("boom".to_string()), JobStatus::Cancelled] {
        let id = queue_with_one_job(&mut queue);
        queue.get_mut(id).unwrap().status = status;
    }
    let queued = queue_with_one_job(&mut queue);

    assert_eq!(queue.startable(4), vec![queued]);
}

#[test]
fn active_jobs_and_active_count_cover_every_in_progress_job() {
    let mut queue = TransferQueue::new();
    let first = queue_with_one_job(&mut queue);
    let second = queue_with_one_job(&mut queue);
    queue_with_one_job(&mut queue);
    queue.get_mut(first).unwrap().status = JobStatus::InProgress;
    queue.get_mut(second).unwrap().status = JobStatus::InProgress;

    let active: Vec<u64> = queue.active_jobs().map(|job| job.id).collect();

    assert_eq!(active, vec![first, second]);
    assert_eq!(queue.active_count(), 2);
}

#[test]
fn active_ids_for_session_returns_only_that_sessions_in_progress_jobs() {
    let mut queue = TransferQueue::new();
    let mine = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    let theirs = enqueue_for(&mut queue, 2, Direction::Upload, "b.txt");
    enqueue_for(&mut queue, 1, Direction::Upload, "c.txt");
    queue.get_mut(mine).unwrap().status = JobStatus::InProgress;
    queue.get_mut(theirs).unwrap().status = JobStatus::InProgress;

    assert_eq!(queue.active_ids_for_session(1), vec![mine]);
}

#[test]
fn has_pending_is_true_for_queued_and_in_progress_jobs() {
    let mut queue = TransferQueue::new();
    let job = enqueue_for(&mut queue, 1, Direction::Download, "a.txt");

    assert!(queue.has_pending(1, Direction::Download));
    queue.get_mut(job).unwrap().status = JobStatus::InProgress;
    assert!(queue.has_pending(1, Direction::Download));
}

#[test]
fn has_pending_ignores_other_sessions_directions_and_finished_jobs() {
    let mut queue = TransferQueue::new();
    let done = enqueue_for(&mut queue, 1, Direction::Download, "done.txt");
    queue.get_mut(done).unwrap().status = JobStatus::Completed;
    enqueue_for(&mut queue, 2, Direction::Download, "other_session.txt");
    enqueue_for(&mut queue, 1, Direction::Upload, "other_direction.txt");

    assert!(!queue.has_pending(1, Direction::Download));
}

#[test]
fn cancel_all_queued_cancels_every_queued_job_and_nothing_else() {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch("batch".to_string());
    let batch_job = queue_with_a_batch_job(&mut queue, batch_id, "a.txt", 10);
    let loose_job = queue_with_one_job(&mut queue);
    let active = queue_with_one_job(&mut queue);
    queue.get_mut(active).unwrap().status = JobStatus::InProgress;

    let count = queue.cancel_all_queued();

    assert_eq!(count, 2);
    assert_eq!(queue.get(batch_job).unwrap().status, JobStatus::Cancelled);
    assert_eq!(queue.get(loose_job).unwrap().status, JobStatus::Cancelled);
    assert_eq!(queue.get(active).unwrap().status, JobStatus::InProgress);
}

#[test]
fn startable_holds_back_a_second_job_for_the_same_destination() {
    let mut queue = TransferQueue::new();
    let first = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    let other = enqueue_for(&mut queue, 1, Direction::Upload, "b.txt");

    assert_eq!(queue.startable(4), vec![first, other]);
}

#[test]
fn startable_waits_while_the_same_destination_is_in_progress() {
    let mut queue = TransferQueue::new();
    let first = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    queue.get_mut(first).unwrap().status = JobStatus::InProgress;
    enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");

    assert!(queue.startable(4).is_empty());
}

#[test]
fn downloads_to_the_same_local_file_from_different_sessions_do_not_overlap() {
    let mut queue = TransferQueue::new();
    let first = enqueue_for(&mut queue, 1, Direction::Download, "a.txt");
    enqueue_for(&mut queue, 2, Direction::Download, "a.txt");

    assert_eq!(queue.startable(4), vec![first]);
}

#[test]
fn uploads_to_the_same_path_on_different_sessions_run_together() {
    let mut queue = TransferQueue::new();
    let first = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    let second = enqueue_for(&mut queue, 2, Direction::Upload, "a.txt");

    assert_eq!(queue.startable(4), vec![first, second]);
}

#[test]
fn get_finds_each_job_by_id_among_many() {
    let mut queue = TransferQueue::new();
    let ids: Vec<u64> = (0..1000).map(|n| enqueue_for(&mut queue, 1, Direction::Upload, &format!("{n}.txt"))).collect();

    for id in ids {
        assert_eq!(queue.get(id).unwrap().id, id);
        assert_eq!(queue.get_mut(id).unwrap().id, id);
    }
    assert!(queue.get(1000).is_none());
}

#[test]
fn start_batch_remembers_its_label() {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch("photos".to_string());

    assert_eq!(queue.batch_label(batch_id), Some("photos"));
    assert_eq!(queue.batch_label(batch_id + 1), None);
}

#[test]
fn jobs_iterates_in_queue_order() {
    let mut queue = TransferQueue::new();
    let first = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    let second = enqueue_for(&mut queue, 1, Direction::Upload, "b.txt");

    let ids: Vec<u64> = queue.jobs().map(|job| job.id).collect();

    assert_eq!(ids, vec![first, second]);
}

#[test]
fn retry_jobs_requeues_only_failed_and_cancelled_jobs_and_resets_them() {
    let mut queue = TransferQueue::new();
    let failed = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    let cancelled = enqueue_for(&mut queue, 1, Direction::Upload, "b.txt");
    let completed = enqueue_for(&mut queue, 1, Direction::Upload, "c.txt");
    let running = enqueue_for(&mut queue, 1, Direction::Upload, "d.txt");
    {
        let job = queue.get_mut(failed).unwrap();
        job.status = JobStatus::Failed("boom".to_string());
        job.attempts = 3;
        job.transferred_bytes = 5;
    }
    queue.get_mut(cancelled).unwrap().status = JobStatus::Cancelled;
    queue.get_mut(completed).unwrap().status = JobStatus::Completed;
    queue.get_mut(running).unwrap().status = JobStatus::InProgress;

    let count = queue.retry_jobs(&[failed, cancelled, completed, running]);

    assert_eq!(count, 2);
    let job = queue.get(failed).unwrap();
    assert_eq!((job.status.clone(), job.attempts, job.transferred_bytes), (JobStatus::Queued, 0, 5));
    assert_eq!(queue.get(cancelled).unwrap().status, JobStatus::Queued);
    assert_eq!(queue.get(completed).unwrap().status, JobStatus::Completed);
    assert_eq!(queue.get(running).unwrap().status, JobStatus::InProgress);
}

#[test]
fn remove_jobs_removes_jobs_and_the_label_of_a_batch_it_emptied() {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch("photos".to_string());
    let a = queue_with_a_batch_job(&mut queue, batch_id, "a.txt", 10);
    let b = queue_with_a_batch_job(&mut queue, batch_id, "b.txt", 10);

    queue.remove_jobs(&[a, b]);

    assert!(queue.get(a).is_none());
    assert!(queue.get(b).is_none());
    assert_eq!(queue.batch_label(batch_id), None);
}

#[test]
fn remove_jobs_keeps_the_label_of_a_batch_that_still_has_jobs() {
    let mut queue = TransferQueue::new();
    let batch_id = queue.start_batch("photos".to_string());
    let a = queue_with_a_batch_job(&mut queue, batch_id, "a.txt", 10);
    queue_with_a_batch_job(&mut queue, batch_id, "b.txt", 10);

    queue.remove_jobs(&[a]);

    assert_eq!(queue.batch_label(batch_id), Some("photos"));
}

#[test]
fn remove_jobs_keeps_labels_of_batches_it_did_not_touch() {
    let mut queue = TransferQueue::new();
    let scanning = queue.start_batch("still scanning".to_string());
    let single = queue_with_one_job(&mut queue);

    queue.remove_jobs(&[single]);

    assert_eq!(queue.batch_label(scanning), Some("still scanning"));
}

#[test]
fn forget_batch_if_empty_drops_only_an_unused_label() {
    let mut queue = TransferQueue::new();
    let unused = queue.start_batch("scan failed".to_string());
    let used = queue.start_batch("photos".to_string());
    queue_with_a_batch_job(&mut queue, used, "a.txt", 10);

    queue.forget_batch_if_empty(unused);
    queue.forget_batch_if_empty(used);

    assert_eq!(queue.batch_label(unused), None);
    assert_eq!(queue.batch_label(used), Some("photos"));
}

#[test]
fn new_jobs_do_not_resume() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);

    assert!(!queue.get(id).unwrap().resume);
}

#[test]
fn an_automatic_retry_resumes() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    queue.get_mut(id).unwrap().status = JobStatus::Failed("boom".to_string());
    queue.get_mut(id).unwrap().attempts = 1;

    assert!(queue.retry_or_give_up(id));

    assert!(queue.get(id).unwrap().resume);
}

#[test]
fn a_manual_retry_resumes() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    queue.get_mut(id).unwrap().status = JobStatus::Cancelled;
    queue.get_mut(id).unwrap().attempts = 1;

    queue.retry_jobs(&[id]);

    assert!(queue.get(id).unwrap().resume);
}

#[test]
fn a_manual_retry_of_a_job_that_never_started_does_not_resume() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    queue.get_mut(id).unwrap().status = JobStatus::Cancelled;

    queue.retry_jobs(&[id]);

    assert!(!queue.get(id).unwrap().resume);
}
