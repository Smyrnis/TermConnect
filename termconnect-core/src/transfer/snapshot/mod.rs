use std::collections::BTreeMap;

use super::{
    Direction, TransferQueue,
    queue::BatchProgress,
    rows::{QueueRow, RowState, ScanInfo, queue_rows},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveJob {
    pub id: u64,
    pub display_name: String,
    pub direction: Direction,
    pub batch_id: Option<u64>,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub percent: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransferSnapshot {
    pub rows: Vec<QueueRow>,
    pub active: Vec<ActiveJob>,
    pub batches: BTreeMap<u64, BatchProgress>,
    pub queued: usize,
    pub scanning: Vec<String>,
    pub awaiting_answers: usize,
}

impl TransferSnapshot {
    pub fn of(queue: &TransferQueue, scans: &[ScanInfo]) -> Self {
        let active: Vec<ActiveJob> = queue
            .active_jobs()
            .map(|job| ActiveJob {
                id: job.id,
                display_name: job.display_name.clone(),
                direction: job.direction,
                batch_id: job.batch_id,
                transferred_bytes: job.transferred_bytes,
                total_bytes: job.total_bytes,
                percent: job.progress_percent(),
            })
            .collect();
        let batches = active
            .iter()
            .filter_map(|job| job.batch_id)
            .map(|batch_id| (batch_id, queue.batch_progress(batch_id)))
            .collect();
        Self {
            rows: queue_rows(queue, scans),
            active,
            batches,
            queued: queue.queued_count(),
            scanning: scans
                .iter()
                .filter(|scan| scan.state == RowState::Scanning)
                .map(|scan| scan.label.to_string())
                .collect(),
            awaiting_answers: scans.iter().filter(|scan| scan.state == RowState::AwaitingAnswer).count(),
        }
    }
}

#[cfg(test)]
mod tests;
