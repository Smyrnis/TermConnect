use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

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
    jobs: BTreeMap<u64, TransferJob>,
    batch_labels: HashMap<u64, String>,
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

        self.jobs.insert(id, TransferJob { id, session_id, direction, local_path, remote_path, display_name, total_bytes, transferred_bytes: 0, status: JobStatus::Queued, attempts: 0, batch_id });

        id
    }

    pub fn start_batch(&mut self, label: String) -> u64 {
        let id = self.next_batch_id;
        self.next_batch_id += 1;
        self.batch_labels.insert(id, label);
        id
    }

    pub fn batch_label(&self, batch_id: u64) -> Option<&str> {
        self.batch_labels.get(&batch_id).map(String::as_str)
    }

    pub fn forget_batch_if_empty(&mut self, batch_id: u64) {
        if !self.jobs.values().any(|job| job.batch_id == Some(batch_id)) {
            self.batch_labels.remove(&batch_id);
        }
    }

    pub fn jobs(&self) -> impl Iterator<Item = &TransferJob> {
        self.jobs.values()
    }

    pub fn retry_jobs(&mut self, ids: &[u64]) -> usize {
        let mut count = 0;
        for id in ids {
            if let Some(job) = self.jobs.get_mut(id)
                && matches!(job.status, JobStatus::Failed(_) | JobStatus::Cancelled)
            {
                job.status = JobStatus::Queued;
                job.attempts = 0;
                job.transferred_bytes = 0;
                count += 1;
            }
        }
        count
    }

    pub fn remove_jobs(&mut self, ids: &[u64]) {
        let touched_batches: HashSet<u64> = ids.iter().filter_map(|id| self.jobs.remove(id)).filter_map(|job| job.batch_id).collect();
        for batch_id in touched_batches {
            self.forget_batch_if_empty(batch_id);
        }
    }

    pub fn get(&self, id: u64) -> Option<&TransferJob> {
        self.jobs.get(&id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut TransferJob> {
        self.jobs.get_mut(&id)
    }

    pub fn active_jobs(&self) -> impl Iterator<Item = &TransferJob> {
        self.jobs.values().filter(|job| job.status == JobStatus::InProgress)
    }

    pub fn active_count(&self) -> usize {
        self.active_jobs().count()
    }

    pub fn startable(&self, limit: usize) -> Vec<u64> {
        let free_slots = limit.saturating_sub(self.active_count());
        let mut busy_destinations: HashSet<Destination> = self.active_jobs().map(TransferJob::destination).collect();
        let mut startable = Vec::new();
        for job in self.jobs.values().filter(|job| job.status == JobStatus::Queued) {
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
        self.jobs.values().any(|job| job.session_id == session_id && job.direction == direction && matches!(job.status, JobStatus::Queued | JobStatus::InProgress))
    }

    pub fn cancel_all_queued(&mut self) -> usize {
        let mut count = 0;
        for job in self.jobs.values_mut().filter(|job| job.status == JobStatus::Queued) {
            job.status = JobStatus::Cancelled;
            count += 1;
        }
        count
    }

    pub fn queued_count(&self) -> usize {
        self.jobs.values().filter(|job| job.status == JobStatus::Queued).count()
    }

    pub fn fail_queued_for_session(&mut self, session_id: u64, reason: &str) -> usize {
        let mut count = 0;
        for job in self.jobs.values_mut() {
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

        for job in self.jobs.values().filter(|job| job.batch_id == Some(batch_id)) {
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
