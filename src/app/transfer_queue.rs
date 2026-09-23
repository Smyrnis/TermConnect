use super::*;
use crate::transfer::rows::{QueueRow, RowKind, RowState, ScanInfo, queue_rows};

impl App {
    pub(super) fn open_transfers_screen(&mut self) {
        self.screen = Screen::Transfers;
        self.transfers_cursor = 0;
    }

    pub(super) fn transfer_rows(&self) -> Vec<QueueRow> {
        let scans: Vec<ScanInfo> = self.planning.iter().map(|scan| ScanInfo { batch_id: scan.batch_id, label: &scan.display_name, direction: scan.direction, state: RowState::Scanning }).chain(self.conflict_reviews.iter().map(|review| ScanInfo { batch_id: review.batch_id, label: self.transfers.batch_label(review.batch_id).unwrap_or("copy"), direction: review.direction, state: RowState::AwaitingAnswer })).collect();
        queue_rows(&self.transfers, &scans)
    }

    pub(super) fn apply_transfers_action(&mut self, action: Action) {
        match action {
            Action::Up => self.transfers_cursor = self.clamped_transfers_cursor().saturating_sub(1),
            Action::Down => {
                let last = self.transfer_rows().len().saturating_sub(1);
                self.transfers_cursor = (self.clamped_transfers_cursor() + 1).min(last);
            }
            Action::Open => self.retry_selected_row(),
            Action::Refresh => self.clear_finished_rows(),
            _ => {}
        }
    }

    pub(super) fn cancel_selected_row(&mut self) {
        let Some(row) = self.selected_transfer_row() else {
            return;
        };
        if let RowKind::Scan(batch_id) = row.kind {
            for scan in self.planning.iter().filter(|scan| scan.batch_id == batch_id) {
                scan.cancel.store(true, Ordering::Relaxed);
            }
            self.drop_conflict_reviews(|review| review.batch_id == batch_id);
            return;
        }
        let mut cancelled_destinations: Vec<(u64, Direction)> = Vec::new();
        for id in &row.job_ids {
            let Some(job) = self.transfers.get_mut(*id) else {
                continue;
            };
            match job.status {
                JobStatus::Queued => {
                    job.status = JobStatus::Cancelled;
                    let destination = (job.session_id, job.direction);
                    if !cancelled_destinations.contains(&destination) {
                        cancelled_destinations.push(destination);
                    }
                }
                JobStatus::InProgress => {
                    if let Some(cancel) = self.transfer_cancels.get(id) {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                _ => {}
            }
        }
        self.refresh_destinations_without_pending(&cancelled_destinations);
    }

    fn retry_selected_row(&mut self) {
        let Some(row) = self.selected_transfer_row() else {
            return;
        };
        let retryable: Vec<&transfer::TransferJob> = row.job_ids.iter().filter_map(|id| self.transfers.get(*id)).filter(|job| matches!(job.status, JobStatus::Failed(_) | JobStatus::Cancelled)).collect();
        let Some(session_id) = retryable.first().map(|job| job.session_id) else {
            return;
        };
        if !self.session_resources.contains_key(&session_id) {
            self.notifications.push(Severity::Warning, "Can't retry: session disconnected");
            return;
        }
        if self.transfers.retry_jobs(&row.job_ids) > 0 {
            self.fill_transfer_slots();
        }
    }

    fn clear_finished_rows(&mut self) {
        let selected_kind = self.selected_transfer_row().map(|row| row.kind);
        let finished_ids: Vec<u64> = self.transfer_rows().into_iter().filter(QueueRow::is_finished).flat_map(|row| row.job_ids).collect();
        self.transfers.remove_jobs(&finished_ids);
        let rows = self.transfer_rows();
        self.transfers_cursor = selected_kind.and_then(|kind| rows.iter().position(|row| row.kind == kind)).unwrap_or_else(|| self.clamped_transfers_cursor());
    }

    fn selected_transfer_row(&self) -> Option<QueueRow> {
        let cursor = self.clamped_transfers_cursor();
        self.transfer_rows().into_iter().nth(cursor)
    }

    fn clamped_transfers_cursor(&self) -> usize {
        self.transfers_cursor.min(self.transfer_rows().len().saturating_sub(1))
    }
}

#[cfg(test)]
#[path = "../../tests/app/transfer_queue_test.rs"]
mod tests;
