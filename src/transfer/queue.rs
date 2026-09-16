use std::path::PathBuf;

use super::job::{Direction, JobStatus, TransferJob};

/// Retries a failed job this many additional times before giving up.
const MAX_ATTEMPTS: u32 = 3;

/// A sequential queue of transfers: at most one job runs at a time, and
/// each finished job (successfully or not) makes room for the next queued
/// one. Completed and permanently-failed jobs stay in the list as history
/// until cleared.
#[derive(Default)]
pub struct TransferQueue {
    jobs: Vec<TransferJob>,
    next_id: u64,
}

impl TransferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(
        &mut self,
        session_id: u64,
        direction: Direction,
        local_path: PathBuf,
        remote_path: String,
        display_name: String,
        total_bytes: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.jobs.push(TransferJob {
            id,
            session_id,
            direction,
            local_path,
            remote_path,
            display_name,
            total_bytes,
            transferred_bytes: 0,
            status: JobStatus::Queued,
            attempts: 0,
        });

        id
    }

    pub fn get(&self, id: u64) -> Option<&TransferJob> {
        self.jobs.iter().find(|job| job.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut TransferJob> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }

    pub fn is_active(&self) -> bool {
        self.jobs
            .iter()
            .any(|job| job.status == JobStatus::InProgress)
    }

    pub fn active(&self) -> Option<&TransferJob> {
        self.jobs
            .iter()
            .find(|job| job.status == JobStatus::InProgress)
    }

    pub fn queued_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|job| job.status == JobStatus::Queued)
            .count()
    }

    /// The next job to run, if any and nothing is currently active.
    pub fn next_to_run(&self) -> Option<u64> {
        if self.is_active() {
            return None;
        }
        self.jobs
            .iter()
            .find(|job| job.status == JobStatus::Queued)
            .map(|job| job.id)
    }

    /// Marks every still-queued job belonging to `session_id` as `Failed`
    /// with `reason`, in one pass. Used when a session disconnects: without
    /// this, each queued job would only fail once `maybe_start_next_transfer`
    /// got to it, pushing its own notification — this lets the caller fail
    /// them all up front and report a single aggregated notification
    /// instead. Deliberately leaves an `InProgress` job for that session
    /// alone — the caller is expected to cancel it separately (see
    /// `App::disconnect_selected`) and let it finish through the normal
    /// transfer-event flow. Returns how many jobs were marked.
    pub fn fail_queued_for_session(&mut self, session_id: u64, reason: &str) -> usize {
        let mut count = 0;
        for job in self.jobs.iter_mut() {
            if job.session_id == session_id && job.status == JobStatus::Queued {
                job.status = JobStatus::Failed(reason.to_string());
                count += 1;
            }
        }
        count
    }

    /// Marks a failed job for retry if it hasn't exhausted its attempts.
    /// Returns `true` if it was re-queued, `false` if it's out of retries
    /// (and stays `Failed`).
    pub fn retry_or_give_up(&mut self, id: u64) -> bool {
        let Some(job) = self.get_mut(id) else {
            return false;
        };

        if job.attempts < MAX_ATTEMPTS {
            job.status = JobStatus::Queued;
            job.transferred_bytes = 0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
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
}
