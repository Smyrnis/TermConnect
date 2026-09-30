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
    let mut job = queue.get_mut(id).unwrap();
    job.status = JobStatus::Failed("boom".to_string());
    job.attempts = 1;
    job.transferred_bytes = 50;
    drop(job);

    assert!(queue.retry_or_give_up(id));
    let job = queue.get(id).unwrap();
    assert_eq!(job.status, JobStatus::Queued);
    assert_eq!(job.transferred_bytes, 50);
}

#[test]
fn retry_or_give_up_stops_after_max_attempts() {
    let mut queue = TransferQueue::new();
    let id = queue_with_one_job(&mut queue);
    let mut job = queue.get_mut(id).unwrap();
    job.status = JobStatus::Failed("boom".to_string());
    job.attempts = 3;
    drop(job);

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
        let mut job = queue.get_mut(failed).unwrap();
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

#[test]
fn a_session_limit_holds_back_that_sessions_jobs_but_not_others() {
    let mut queue = TransferQueue::new();
    let first = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    enqueue_for(&mut queue, 1, Direction::Upload, "b.txt");
    let other = enqueue_for(&mut queue, 2, Direction::Upload, "c.txt");

    let limits = |session: u64| (session == 1).then_some(1);

    assert_eq!(queue.startable_limited(4, limits), vec![first, other]);
}

#[test]
fn a_session_limit_counts_that_sessions_active_jobs() {
    let mut queue = TransferQueue::new();
    let active = enqueue_for(&mut queue, 1, Direction::Upload, "a.txt");
    queue.get_mut(active).unwrap().status = JobStatus::InProgress;
    enqueue_for(&mut queue, 1, Direction::Upload, "b.txt");

    assert!(queue.startable_limited(4, |_| Some(1)).is_empty());
    assert_eq!(queue.startable_limited(4, |_| Some(2)).len(), 1);
}

use std::time::Instant;

use super::super::rows::{QueueRow, RowKind, RowState};

fn reference_rows(queue: &TransferQueue) -> Vec<QueueRow> {
    let mut groups: Vec<Vec<&TransferJob>> = Vec::new();
    let mut batch_group: HashMap<u64, usize> = HashMap::new();
    for job in queue.jobs() {
        match job.batch_id {
            Some(batch_id) => match batch_group.get(&batch_id) {
                Some(&index) => groups[index].push(job),
                None => {
                    batch_group.insert(batch_id, groups.len());
                    groups.push(vec![job]);
                }
            },
            None => groups.push(vec![job]),
        }
    }
    groups
        .iter()
        .map(|jobs| {
            let first = jobs[0];
            let (kind, label) = match first.batch_id {
                Some(batch_id) => {
                    (RowKind::Batch(batch_id), queue.batch_label(batch_id).unwrap_or(&first.display_name).to_string())
                }
                None => (RowKind::Single(first.id), first.display_name.clone()),
            };
            let completed = jobs.iter().filter(|job| job.status == JobStatus::Completed).count();
            let failed = jobs.iter().filter(|job| matches!(job.status, JobStatus::Failed(_))).count();
            let state = if jobs.iter().any(|job| job.status == JobStatus::InProgress) {
                RowState::Running
            } else if jobs.iter().any(|job| job.status == JobStatus::Queued) {
                RowState::Queued
            } else if completed == jobs.len() {
                RowState::Done
            } else if failed == 0 {
                RowState::Cancelled
            } else if completed > 0 {
                RowState::PartlyFailed(failed)
            } else {
                RowState::Failed
            };
            QueueRow {
                kind,
                label,
                direction: first.direction,
                files_done: completed,
                files_total: jobs.len(),
                bytes_done: jobs.iter().map(|job| job.transferred_bytes).sum(),
                bytes_total: jobs.iter().map(|job| job.total_bytes).sum(),
                state,
            }
        })
        .collect()
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound
    }
}

fn add_job(queue: &mut TransferQueue, name: &str, batch: Option<u64>, session: u64, direction: Direction) -> u64 {
    queue.enqueue(
        session,
        direction,
        PathBuf::from(format!("/local/{name}")),
        format!("/remote/{name}"),
        name.to_string(),
        100,
        batch,
    )
}

fn reference_startable(queue: &TransferQueue, limit: usize, session_limit: impl Fn(u64) -> Option<usize>) -> Vec<u64> {
    let running = |job: &&TransferJob| job.status == JobStatus::InProgress;
    let free_slots = limit.saturating_sub(queue.jobs().filter(running).count());
    let mut busy_destinations: HashSet<Destination> =
        queue.jobs().filter(running).map(TransferJob::destination).collect();
    let mut per_session: HashMap<u64, usize> = HashMap::new();
    for job in queue.jobs().filter(running) {
        *per_session.entry(job.session_id).or_default() += 1;
    }
    let mut startable = Vec::new();
    for job in queue.jobs().filter(|job| job.status == JobStatus::Queued) {
        if startable.len() == free_slots {
            break;
        }
        let in_flight = per_session.get(&job.session_id).copied().unwrap_or(0);
        if session_limit(job.session_id).is_some_and(|cap| in_flight >= cap) {
            continue;
        }
        if busy_destinations.insert(job.destination()) {
            *per_session.entry(job.session_id).or_default() += 1;
            startable.push(job.id);
        }
    }
    startable
}

fn finished_map(rows: &[QueueRow]) -> HashMap<RowKind, bool> {
    rows.iter().map(|row| (row.kind, row.is_finished())).collect()
}

fn pick(random: &mut Lcg, ids: &[u64]) -> Option<u64> {
    if ids.is_empty() { None } else { Some(ids[random.next(ids.len() as u64) as usize]) }
}

#[test]
fn everything_matches_a_recount_from_the_jobs_after_every_random_operation() {
    let mut queue = TransferQueue::new();
    let mut random = Lcg(7);
    let batches: Vec<u64> = (0..3).map(|index| queue.start_batch(format!("batch {index}"))).collect();
    let mut ids: Vec<u64> = Vec::new();
    let mut removals = 0;
    let mut previous = finished_map(&queue.rows());
    for step in 0..1200u64 {
        match random.next(11) {
            0 | 1 => {
                let batch = match random.next(4) {
                    3 => None,
                    index => Some(batches[index as usize]),
                };
                let direction = if random.next(2) == 0 { Direction::Upload } else { Direction::Download };
                ids.push(add_job(&mut queue, &format!("f{step}"), batch, random.next(3), direction));
            }
            2 | 3 => {
                if let Some(id) = pick(&mut random, &ids) {
                    let status = match random.next(5) {
                        0 => JobStatus::Queued,
                        1 => JobStatus::InProgress,
                        2 => JobStatus::Completed,
                        3 => JobStatus::Failed("boom".to_string()),
                        _ => JobStatus::Cancelled,
                    };
                    if let Some(mut job) = queue.get_mut(id) {
                        job.status = status;
                    }
                }
            }
            4 => {
                if let Some(id) = pick(&mut random, &ids)
                    && let Some(mut job) = queue.get_mut(id)
                {
                    job.transferred_bytes = random.next(101);
                }
            }
            5 => {
                let chosen: Vec<u64> = ids.iter().copied().filter(|_| random.next(4) == 0).collect();
                queue.retry_jobs(&chosen);
            }
            6 => {
                queue.cancel_all_queued();
            }
            7 => {
                queue.fail_queued_for_session(random.next(3), "gone");
            }
            8 => {
                if let Some(id) = pick(&mut random, &ids) {
                    queue.retry_or_give_up(id);
                }
            }
            9 => {
                if let Some(id) = pick(&mut random, &ids)
                    && let Some(mut job) = queue.get_mut(id)
                {
                    job.session_id = random.next(3);
                }
            }
            _ => {
                let whole_batch = random.next(3) == 0;
                let chosen: Vec<u64> = if whole_batch {
                    let batch = batches[random.next(3) as usize];
                    queue.jobs().filter(|job| job.batch_id == Some(batch)).map(|job| job.id).collect()
                } else {
                    ids.iter().copied().filter(|_| random.next(6) == 0).collect()
                };
                removals += chosen.len();
                queue.remove_jobs(&chosen);
                ids.retain(|id| queue.get(*id).is_some());
            }
        }
        let rows = queue.rows();
        assert_eq!(rows, reference_rows(&queue), "rows differ after step {step}");
        queue.assert_consistent();
        let changes = queue.take_changes();
        let current = finished_map(&rows);
        for (kind, was_finished) in &previous {
            match current.get(kind) {
                Some(now) if now != was_finished => assert!(
                    changes
                        .iter()
                        .any(|change| matches!(change, RowChange::Finished(k) | RowChange::Reopened(k) if k == kind)),
                    "{kind:?} flipped without a change after step {step}: {changes:?}"
                ),
                None => assert!(
                    changes.contains(&RowChange::Removed(*kind)),
                    "{kind:?} vanished without a Removed change after step {step}: {changes:?}"
                ),
                _ => {}
            }
        }
        previous = current;
        for session in 0..3 {
            for direction in [Direction::Upload, Direction::Download] {
                let expected = queue.jobs().any(|job| {
                    job.session_id == session
                        && job.direction == direction
                        && matches!(job.status, JobStatus::Queued | JobStatus::InProgress)
                });
                assert_eq!(queue.has_pending(session, direction), expected, "pending after step {step}");
            }
        }
        assert_eq!(queue.queued_count(), queue.jobs().filter(|job| job.status == JobStatus::Queued).count());
        for batch in &batches {
            let jobs: Vec<&TransferJob> = queue.jobs().filter(|job| job.batch_id == Some(*batch)).collect();
            let progress = queue.batch_progress(*batch);
            assert_eq!(progress.total_files, jobs.len(), "batch files after step {step}");
            assert_eq!(progress.completed_files, jobs.iter().filter(|job| job.status == JobStatus::Completed).count());
            assert_eq!(progress.total_bytes, jobs.iter().map(|job| job.total_bytes).sum::<u64>());
            assert_eq!(progress.transferred_bytes, jobs.iter().map(|job| job.transferred_bytes).sum::<u64>());
        }
        let limit_of = |session: u64| match session {
            0 => Some(2),
            1 => Some(4),
            _ => None,
        };
        for limit in [1, 3, 8] {
            assert_eq!(
                queue.startable_limited(limit, limit_of),
                reference_startable(&queue, limit, limit_of),
                "startable at limit {limit} after step {step}"
            );
        }
    }
    assert!(removals > 20, "the random run removed only {removals} jobs");
}

#[test]
fn removing_the_last_unfinished_job_of_a_row_finishes_it() {
    let mut queue = TransferQueue::new();
    let batch = queue.start_batch("docs".to_string());
    let done = add_job(&mut queue, "a", Some(batch), 1, Direction::Upload);
    let waiting = add_job(&mut queue, "b", Some(batch), 1, Direction::Upload);
    queue.get_mut(done).unwrap().status = JobStatus::Completed;
    drain(&mut queue);

    queue.remove_jobs(&[waiting]);

    assert_eq!(drain(&mut queue), vec![RowChange::Finished(RowKind::Batch(batch))]);
    assert_eq!(queue.row(RowKind::Batch(batch)).unwrap().state, RowState::Done);
}

#[test]
fn a_row_keeps_the_order_and_label_of_its_first_remaining_job_after_a_partial_removal() {
    let mut queue = TransferQueue::new();
    let first = add_job(&mut queue, "first", Some(99), 1, Direction::Upload);
    let single = add_job(&mut queue, "single", None, 1, Direction::Upload);
    add_job(&mut queue, "second", Some(99), 1, Direction::Download);

    queue.remove_jobs(&[first]);

    let rows = queue.rows();
    assert_eq!(rows.iter().map(|row| row.kind).collect::<Vec<_>>(), vec![RowKind::Single(single), RowKind::Batch(99)]);
    assert_eq!(rows[1].label, "second");
    assert_eq!(rows[1].direction, Direction::Download);
    queue.assert_consistent();
}

#[test]
#[should_panic(expected = "row counters differ")]
fn the_consistency_check_notices_a_corrupt_counter() {
    let mut queue = TransferQueue::new();
    add_job(&mut queue, "a", None, 1, Direction::Upload);

    queue.corrupt_for_test();

    queue.assert_consistent();
}

#[test]
fn starting_does_not_walk_the_queue_of_a_capped_session() {
    fn per_call(jobs: usize) -> f64 {
        let mut queue = TransferQueue::new();
        for index in 0..2 {
            let id = add_job(&mut queue, &format!("r{index}"), None, 1, Direction::Upload);
            queue.get_mut(id).unwrap().status = JobStatus::InProgress;
        }
        for index in 0..jobs {
            add_job(&mut queue, &format!("a{index}"), None, 1, Direction::Download);
        }
        for index in 0..jobs {
            add_job(&mut queue, &format!("b{index}"), None, 2, Direction::Download);
        }
        let calls = 300;
        let start = Instant::now();
        for _ in 0..calls {
            std::hint::black_box(queue.startable_limited(4, |session| if session == 1 { Some(2) } else { None }));
        }
        start.elapsed().as_secs_f64() / calls as f64
    }

    let small = (0..3).map(|_| per_call(1_000)).fold(f64::MAX, f64::min);
    let large = (0..3).map(|_| per_call(100_000)).fold(f64::MAX, f64::min);

    assert!(large < small * 30.0, "per call: {small:.9}s with 1k queued, {large:.9}s with 100k queued");
}

#[test]
fn a_row_is_looked_up_by_kind_and_keeps_its_jobs_in_order() {
    let mut queue = TransferQueue::new();
    let batch = queue.start_batch("docs".to_string());
    let a = add_job(&mut queue, "a", Some(batch), 1, Direction::Upload);
    let single = add_job(&mut queue, "s", None, 1, Direction::Upload);
    let b = add_job(&mut queue, "b", Some(batch), 1, Direction::Upload);

    assert_eq!(queue.row(RowKind::Batch(batch)).unwrap().files_total, 2);
    assert_eq!(queue.row(RowKind::Single(single)).unwrap().label, "s");
    assert!(queue.row(RowKind::Single(a)).is_none());
    assert!(queue.row(RowKind::Scan(3)).is_none());
    assert_eq!(queue.jobs_of(RowKind::Batch(batch)).iter().map(|job| job.id).collect::<Vec<_>>(), vec![a, b]);
    assert!(queue.jobs_of(RowKind::Batch(99)).is_empty());
}

fn drain(queue: &mut TransferQueue) -> Vec<RowChange> {
    queue.take_changes()
}

#[test]
fn a_row_reports_when_it_finishes_and_when_it_reopens() {
    let mut queue = TransferQueue::new();
    let batch = queue.start_batch("docs".to_string());
    let a = add_job(&mut queue, "a", Some(batch), 1, Direction::Upload);
    let b = add_job(&mut queue, "b", Some(batch), 1, Direction::Upload);
    assert!(drain(&mut queue).is_empty());

    queue.get_mut(a).unwrap().status = JobStatus::Completed;
    assert!(drain(&mut queue).is_empty());
    queue.get_mut(b).unwrap().status = JobStatus::Failed("boom".to_string());
    assert_eq!(drain(&mut queue), vec![RowChange::Finished(RowKind::Batch(batch))]);

    queue.retry_jobs(&[b]);
    assert_eq!(drain(&mut queue), vec![RowChange::Reopened(RowKind::Batch(batch))]);
    assert!(drain(&mut queue).is_empty());
}

#[test]
fn cancelling_and_failing_queued_jobs_finish_their_rows() {
    let mut queue = TransferQueue::new();
    let single = add_job(&mut queue, "one", None, 1, Direction::Upload);
    let other = add_job(&mut queue, "two", None, 2, Direction::Upload);

    queue.cancel_all_queued();
    let mut changes = drain(&mut queue);
    changes.sort_by_key(|change| format!("{change:?}"));
    assert_eq!(
        changes,
        vec![RowChange::Finished(RowKind::Single(single)), RowChange::Finished(RowKind::Single(other))]
    );

    queue.retry_jobs(&[single, other]);
    drain(&mut queue);
    queue.fail_queued_for_session(1, "session disconnected");
    assert_eq!(drain(&mut queue), vec![RowChange::Finished(RowKind::Single(single))]);
}

#[test]
fn removing_every_job_of_a_row_reports_it_removed() {
    let mut queue = TransferQueue::new();
    let batch = queue.start_batch("docs".to_string());
    let a = add_job(&mut queue, "a", Some(batch), 1, Direction::Upload);
    let b = add_job(&mut queue, "b", Some(batch), 1, Direction::Upload);
    let keep = add_job(&mut queue, "k", None, 1, Direction::Upload);
    for id in [a, b] {
        queue.get_mut(id).unwrap().status = JobStatus::Completed;
    }
    drain(&mut queue);

    queue.remove_jobs(&[a]);
    assert!(drain(&mut queue).is_empty());
    queue.remove_jobs(&[b]);

    assert_eq!(drain(&mut queue), vec![RowChange::Removed(RowKind::Batch(batch))]);
    assert!(queue.row(RowKind::Batch(batch)).is_none());
    assert!(queue.batch_label(batch).is_none());
    assert_eq!(queue.rows().len(), 1);
    assert!(queue.get(keep).is_some());
}

#[test]
fn a_job_that_moves_to_another_session_keeps_the_indexes_right() {
    let mut queue = TransferQueue::new();
    let id = add_job(&mut queue, "a", None, 1, Direction::Upload);
    assert!(queue.has_pending(1, Direction::Upload));

    queue.get_mut(id).unwrap().session_id = 2;

    assert!(!queue.has_pending(1, Direction::Upload));
    assert!(queue.has_pending(2, Direction::Upload));
    queue.assert_consistent();
}

#[test]
fn starting_looks_only_at_queued_jobs_and_stops_when_every_session_is_capped() {
    let mut queue = TransferQueue::new();
    let running: Vec<u64> =
        (0..3).map(|index| add_job(&mut queue, &format!("r{index}"), None, 1, Direction::Upload)).collect();
    for id in &running {
        queue.get_mut(*id).unwrap().status = JobStatus::InProgress;
    }
    for index in 0..50 {
        add_job(&mut queue, &format!("q{index}"), None, 1, Direction::Download);
    }

    assert!(queue.startable_limited(10, |_| Some(3)).is_empty());
    assert_eq!(queue.startable_limited(10, |_| Some(5)).len(), 2);
    assert_eq!(queue.startable_limited(5, |_| None).len(), 2);
}

#[test]
fn the_cost_of_a_snapshot_does_not_grow_with_the_number_of_jobs() {
    fn per_event(jobs: usize) -> f64 {
        let mut queue = TransferQueue::new();
        let batch = queue.start_batch("big".to_string());
        let ids: Vec<u64> = (0..jobs)
            .map(|index| add_job(&mut queue, &format!("f{index}"), Some(batch), 1, Direction::Upload))
            .collect();
        let events = 2000usize;
        let start = Instant::now();
        for index in 0..events {
            let id = ids[index % ids.len()];
            let previous = ids[(index + ids.len() - 1) % ids.len()];
            queue.get_mut(previous).unwrap().status = JobStatus::Queued;
            queue.get_mut(id).unwrap().status = JobStatus::InProgress;
            queue.startable_limited(16, |_| Some(6));
            queue.has_pending(1, Direction::Upload);
            std::hint::black_box(crate::transfer::TransferSnapshot::of(&queue, &[]));
            queue.take_changes();
        }
        start.elapsed().as_secs_f64() / events as f64
    }

    let small = (0..3).map(|_| per_event(1_000)).fold(f64::MAX, f64::min);
    let large = (0..3).map(|_| per_event(100_000)).fold(f64::MAX, f64::min);

    assert!(large < small * 30.0, "per event: {small:.9}s at 1k jobs, {large:.9}s at 100k jobs");
}
