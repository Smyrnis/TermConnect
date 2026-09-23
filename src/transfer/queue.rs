use std::path::PathBuf;

use super::job::{Direction, JobStatus, TransferJob};

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
    pub fn enqueue(
        &mut self, session_id: u64, direction: Direction, local_path: PathBuf, remote_path: String,
        display_name: String, total_bytes: u64, batch_id: Option<u64>,
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
        self.jobs.iter().any(|job| job.status == JobStatus::InProgress)
    }

    pub fn active(&self) -> Option<&TransferJob> {
        self.jobs.iter().find(|job| job.status == JobStatus::InProgress)
    }

    pub fn queued_count(&self) -> usize {
        self.jobs.iter().filter(|job| job.status == JobStatus::Queued).count()
    }

    pub fn next_to_run(&self) -> Option<u64> {
        if self.is_active() {
            return None;
        }
        self.jobs.iter().find(|job| job.status == JobStatus::Queued).map(|job| job.id)
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
