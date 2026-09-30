use super::{Direction, TransferQueue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowKind {
    Single(u64),
    Batch(u64),
    Scan(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Scanning,
    AwaitingAnswer,
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
    pub state: RowState,
}

pub fn queue_rows(queue: &TransferQueue, scans: &[ScanInfo]) -> Vec<QueueRow> {
    let mut rows = queue.rows();
    rows.extend(scans.iter().map(scan_row));
    rows
}

pub fn percent_of(done: u64, total: u64) -> u8 {
    if total == 0 { 100 } else { ((done as f64 / total as f64) * 100.0) as u8 }
}

fn scan_row(scan: &ScanInfo) -> QueueRow {
    QueueRow {
        kind: RowKind::Scan(scan.batch_id),
        label: scan.label.to_string(),
        direction: scan.direction,
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        state: scan.state,
    }
}

#[cfg(test)]
mod tests;
