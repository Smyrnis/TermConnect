use std::collections::HashMap;

use super::{Direction, JobStatus, TransferJob, TransferQueue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Single(u64),
    Batch(u64),
    Scan(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Scanning,
    Running,
    Queued,
    Done,
    PartlyFailed(usize),
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    pub kind: RowKind,
    pub label: String,
    pub direction: Direction,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub state: RowState,
    pub job_ids: Vec<u64>,
}

impl QueueRow {
    pub fn is_finished(&self) -> bool {
        Self::finished_state(&self.state)
    }

    pub fn finished_state(state: &RowState) -> bool {
        matches!(state, RowState::Done | RowState::PartlyFailed(_) | RowState::Failed | RowState::Cancelled)
    }
}

pub struct ScanInfo<'a> {
    pub batch_id: u64,
    pub label: &'a str,
    pub direction: Direction,
}

pub fn queue_rows(queue: &TransferQueue, scans: &[ScanInfo]) -> Vec<QueueRow> {
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

    let mut rows: Vec<QueueRow> = groups.iter().map(|jobs| row_for_jobs(queue, jobs)).collect();
    rows.extend(scans.iter().map(scan_row));
    rows
}

pub fn percent_of(done: u64, total: u64) -> u8 {
    if total == 0 { 100 } else { ((done as f64 / total as f64) * 100.0) as u8 }
}

fn row_for_jobs(queue: &TransferQueue, jobs: &[&TransferJob]) -> QueueRow {
    let first = jobs[0];
    let (kind, label) = match first.batch_id {
        Some(batch_id) => (RowKind::Batch(batch_id), queue.batch_label(batch_id).unwrap_or(&first.display_name).to_string()),
        None => (RowKind::Single(first.id), first.display_name.clone()),
    };
    QueueRow { kind, label, direction: first.direction, files_done: jobs.iter().filter(|job| job.status == JobStatus::Completed).count(), files_total: jobs.len(), bytes_done: jobs.iter().map(|job| job.transferred_bytes).sum(), bytes_total: jobs.iter().map(|job| job.total_bytes).sum(), state: row_state(jobs), job_ids: jobs.iter().map(|job| job.id).collect() }
}

fn row_state(jobs: &[&TransferJob]) -> RowState {
    let completed = jobs.iter().filter(|job| job.status == JobStatus::Completed).count();
    let failed = jobs.iter().filter(|job| matches!(job.status, JobStatus::Failed(_))).count();
    if jobs.iter().any(|job| job.status == JobStatus::InProgress) {
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
    }
}

fn scan_row(scan: &ScanInfo) -> QueueRow {
    QueueRow { kind: RowKind::Scan(scan.batch_id), label: scan.label.to_string(), direction: scan.direction, files_done: 0, files_total: 0, bytes_done: 0, bytes_total: 0, state: RowState::Scanning, job_ids: Vec::new() }
}

#[cfg(test)]
#[path = "../../tests/transfer/rows_test.rs"]
mod tests;
