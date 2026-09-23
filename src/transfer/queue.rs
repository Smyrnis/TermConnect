use std::path::PathBuf;

use std::collections::HashSet;

use super::job::{Destination, Direction, JobStatus, TransferJob};

const MAX_ATTEMPTS: u32 = 3;

pub struct BatchProgress {
    pub total_files: usize,
    pub completed_files: usize,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
}

#[derive(Default)]
pub struct TransferQueue {
    jobs: Vec<TransferJob>,
    next_id: u64,
    next_batch_id: u64,
}

impl TransferQueue {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue(&mut self, session_id: u64, direction: Direction, local_path: PathBuf, remote_path: String, display_name: String, total_bytes: u64, batch_id: Option<u64>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.jobs.push(TransferJob { id, session_id, direction, local_path, remote_path, display_name, total_bytes, transferred_bytes: 0, status: JobStatus::Queued, attempts: 0, batch_id });

        id
    }

    pub fn start_batch(&mut self) -> u64 {
        let id = self.next_batch_id;
        self.next_batch_id += 1;
        id
    }

    pub fn get(&self, id: u64) -> Option<&TransferJob> {
        usize::try_from(id).ok().and_then(|index| self.jobs.get(index)).filter(|job| job.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut TransferJob> {
        usize::try_from(id).ok().and_then(|index| self.jobs.get_mut(index)).filter(|job| job.id == id)
    }

    pub fn active_jobs(&self) -> impl Iterator<Item = &TransferJob> {
        self.jobs.iter().filter(|job| job.status == JobStatus::InProgress)
    }

    pub fn active_count(&self) -> usize {
        self.active_jobs().count()
    }

    pub fn startable(&self, limit: usize) -> Vec<u64> {
        let free_slots = limit.saturating_sub(self.active_count());
        let mut busy_destinations: HashSet<Destination> = self.active_jobs().map(TransferJob::destination).collect();
        let mut startable = Vec::new();
        for job in self.jobs.iter().filter(|job| job.status == JobStatus::Queued) {
            if startable.len() == free_slots {
                break;
            }
            if busy_destinations.insert(job.destination()) {
                startable.push(job.id);
            }
        }
        startable
    }

    pub fn active_ids_for_session(&self, session_id: u64) -> Vec<u64> {
        self.active_jobs().filter(|job| job.session_id == session_id).map(|job| job.id).collect()
    }

    pub fn has_pending(&self, session_id: u64, direction: Direction) -> bool {
        self.jobs.iter().any(|job| job.session_id == session_id && job.direction == direction && matches!(job.status, JobStatus::Queued | JobStatus::InProgress))
    }

    pub fn cancel_all_queued(&mut self) -> usize {
        let mut count = 0;
        for job in self.jobs.iter_mut().filter(|job| job.status == JobStatus::Queued) {
            job.status = JobStatus::Cancelled;
            count += 1;
        }
        count
    }

    pub fn queued_count(&self) -> usize {
        self.jobs.iter().filter(|job| job.status == JobStatus::Queued).count()
    }

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

    pub fn batch_progress(&self, batch_id: u64) -> BatchProgress {
        let mut progress = BatchProgress { total_files: 0, completed_files: 0, total_bytes: 0, transferred_bytes: 0 };

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
}

#[cfg(test)]
#[path = "../../tests/transfer/queue_test.rs"]
mod tests;
