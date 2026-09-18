use std::path::PathBuf;

use super::job::{Direction, JobStatus, TransferJob};

/// Retries a failed job this many additional times before giving up.
const MAX_ATTEMPTS: u32 = 3;

pub struct BatchProgress {
    pub total_files: usize,
    pub completed_files: usize,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
}

/// A sequential queue of transfers: at most one job runs at a time, and
/// each finished job (successfully or not) makes room for the next queued
/// one. Completed and permanently-failed jobs stay in the list as history
/// until cleared.
#[derive(Default)]
pub struct TransferQueue {
    jobs: Vec<TransferJob>,
    next_id: u64,
    #[allow(dead_code)]
    next_batch_id: u64,
}

impl TransferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue(
        &mut self,
        session_id: u64,
        direction: Direction,
        local_path: PathBuf,
        remote_path: String,
        display_name: String,
        total_bytes: u64,
        batch_id: Option<u64>,
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
            batch_id,
        });

        id
    }

    /// Mints a new batch id, shared by every `TransferJob` spawned from one
    /// directory copy (see `enqueue`'s `batch_id` parameter).
    #[allow(dead_code)]
    pub fn start_batch(&mut self) -> u64 {
        let id = self.next_batch_id;
        self.next_batch_id += 1;
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

    /// Aggregates every job sharing `batch_id` — used to show combined
    /// progress for a directory copy in the status line.
    pub fn batch_progress(&self, batch_id: u64) -> BatchProgress {
        let mut progress = BatchProgress {
            total_files: 0,
            completed_files: 0,
            total_bytes: 0,
            transferred_bytes: 0,
        };

        for job in self.jobs.iter().filter(|job| job.batch_id == Some(batch_id)) {
            progress.total_files += 1;
            progress.total_bytes += job.total_bytes;
            progress.transferred_bytes += job.transferred_bytes;
            if job.status == JobStatus::Completed {
                progress.completed_files += 1;
            }
        }

        progress
    }

    /// Marks every still-`Queued` job in `batch_id` as `Cancelled`, in the
    /// same style as `fail_queued_for_session`. Leaves any `InProgress` job
    /// alone — the caller cancels that one separately via its own
    /// `AtomicBool`, exactly like a single-file transfer already does.
    /// Returns how many jobs were cancelled.
    pub fn cancel_batch(&mut self, batch_id: u64) -> usize {
        let mut count = 0;
        for job in self.jobs.iter_mut() {
            if job.batch_id == Some(batch_id) && job.status == JobStatus::Queued {
                job.status = JobStatus::Cancelled;
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
#[path = "../../tests/transfer/queue_test.rs"]
mod tests;
