use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};

use super::super::{Engine, Event};
use crate::{
    Severity,
    history::{History, HistoryEntry, HistoryResult, MAX_FAILED_FILES, render_entries},
    transfer::{
        Direction, JobStatus, RowChange, TransferJob,
        rows::{QueueRow, RowKind, RowState},
    },
};

pub(crate) struct HistoryLog {
    store: History,
    recorded: HashSet<RowKind>,
    counted: HashMap<RowKind, HashSet<u64>>,
    write_warned: bool,
    write_seq: u64,
    clear_tag: Option<u64>,
    session_names: HashMap<u64, String>,
}

impl HistoryLog {
    pub(crate) fn new(store: History) -> Self {
        Self {
            store,
            recorded: HashSet::new(),
            counted: HashMap::new(),
            write_warned: false,
            write_seq: 0,
            clear_tag: None,
            session_names: HashMap::new(),
        }
    }

    pub(crate) fn remember_session(&mut self, session_id: u64, name: String) {
        self.session_names.insert(session_id, name);
    }
}

fn common_directory<'a>(paths: impl Iterator<Item = &'a Path>) -> PathBuf {
    let mut common: Option<PathBuf> = None;
    for path in paths {
        let parent = path.parent().unwrap_or(path);
        common = Some(match common {
            None => parent.to_path_buf(),
            Some(current) => current
                .components()
                .zip(parent.components())
                .take_while(|(left, right)| left == right)
                .map(|(component, _)| component)
                .collect::<PathBuf>(),
        });
    }
    common.unwrap_or_default()
}

fn history_result(state: RowState) -> HistoryResult {
    match state {
        RowState::Done => HistoryResult::Done,
        RowState::PartlyFailed(failed) => HistoryResult::PartlyFailed { failed },
        RowState::Failed => HistoryResult::Failed,
        RowState::Cancelled => HistoryResult::Cancelled,
        RowState::Scanning | RowState::AwaitingAnswer | RowState::Running | RowState::Queued => {
            HistoryResult::Interrupted
        }
    }
}

fn new_entry(
    finished_at: DateTime<Utc>, connection: String, direction: Direction, label: String, result: HistoryResult,
) -> HistoryEntry {
    HistoryEntry {
        finished_at,
        connection,
        direction,
        label,
        local_path: String::new(),
        remote_path: String::new(),
        files_done: 0,
        files_total: 0,
        bytes: 0,
        result,
        failed_count: 0,
        failed_files: Vec::new(),
    }
}

impl Engine {
    pub(crate) fn publish_history(&self) {
        self.emit(Event::History(self.history.store.newest_first()));
    }

    pub(crate) fn clear_history(&mut self) {
        self.history.store.clear_memory();
        self.persist_history(true);
        self.publish_history();
    }

    fn session_name(&self, session_id: u64) -> String {
        self.sessions
            .get(&session_id)
            .map(|session| session.name.clone())
            .or_else(|| self.history.session_names.get(&session_id).cloned())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn scan_session(&self, batch_id: u64) -> Option<u64> {
        self.planning
            .iter()
            .find(|scan| scan.batch_id == batch_id)
            .map(|scan| scan.session_id)
            .or_else(|| self.reviews.iter().find(|review| review.batch_id == batch_id).map(|review| review.session_id))
    }

    pub(crate) fn store_entry(&mut self, entry: HistoryEntry) {
        self.history.store.push(entry);
        self.persist_history(false);
    }

    fn persist_history(&mut self, clearing: bool) {
        self.history.write_seq += 1;
        let tag = self.history.write_seq;
        if clearing {
            self.history.clear_tag = Some(tag);
        }
        if !self.history.store.is_writable() {
            self.report_history_failure(
                "the existing history file could not be read or set aside, so it was left alone".to_string(),
                clearing,
            );
            return;
        }
        let snapshot = self.history.store.snapshot();
        let path = self.history.store.path().to_path_buf();
        let render = move || render_entries(&snapshot).map(String::into_bytes).map_err(|err| format!("{err:#}"));
        if let Err(message) = self.writer.write_lazy(path, 0o600, tag, render) {
            self.report_history_failure(message, clearing);
        }
    }

    pub(crate) fn persist_failed(&mut self, _path: PathBuf, message: String, tag: u64) {
        let clearing = self.history.clear_tag == Some(tag);
        self.report_history_failure(message, clearing);
    }

    fn report_history_failure(&mut self, message: String, clearing: bool) {
        if clearing {
            self.report(Severity::Warning, format!("Couldn't clear transfer history: {message}"));
        } else if !self.history.write_warned {
            self.history.write_warned = true;
            self.report(Severity::Warning, format!("Couldn't save transfer history: {message}"));
        } else {
            tracing::debug!("{message}");
        }
    }

    pub(crate) fn process_row_changes(&mut self) {
        let changes = self.transfers.take_changes();
        if changes.is_empty() {
            return;
        }
        let mut seen: HashSet<RowKind> = HashSet::new();
        let mut kinds: Vec<RowKind> = Vec::new();
        for change in changes {
            let kind = match change {
                RowChange::Finished(kind) | RowChange::Reopened(kind) | RowChange::Removed(kind) => kind,
            };
            if seen.insert(kind) {
                kinds.push(kind);
            }
        }
        let mut recorded_any = false;
        for kind in kinds {
            match self.transfers.row(kind) {
                None => {
                    self.history.recorded.remove(&kind);
                    self.history.counted.remove(&kind);
                }
                Some(row) if row.is_finished() => {
                    if self.history.recorded.insert(kind) {
                        let entry = self.entry_for_row(&row);
                        self.log_failed_jobs(&row, &entry.connection);
                        let completed = self.completed_ids(&row);
                        self.history.counted.insert(kind, completed);
                        self.store_entry(entry);
                        recorded_any = true;
                    }
                }
                Some(_) => {
                    self.history.recorded.remove(&kind);
                }
            }
        }
        if recorded_any {
            self.publish_history();
        }
    }

    fn completed_ids(&self, row: &QueueRow) -> HashSet<u64> {
        self.transfers
            .jobs_of(row.kind)
            .into_iter()
            .filter(|job| job.status == JobStatus::Completed)
            .map(|job| job.id)
            .collect()
    }

    pub(crate) fn entry_for_row(&self, row: &QueueRow) -> HistoryEntry {
        let earlier = self.history.counted.get(&row.kind);
        let jobs: Vec<&TransferJob> = self
            .transfers
            .jobs_of(row.kind)
            .into_iter()
            .filter(|job| earlier.is_none_or(|counted| !counted.contains(&job.id)))
            .collect();
        let session_id = jobs.first().map(|job| job.session_id).or_else(|| match row.kind {
            RowKind::Scan(batch_id) => self.scan_session(batch_id),
            _ => None,
        });
        let connection = session_id.map(|id| self.session_name(id)).unwrap_or_else(|| "unknown".to_string());
        let (local_path, remote_path) = match jobs.as_slice() {
            [job] if job.display_name == row.label => (job.local_path.clone(), job.remote_path.clone()),
            _ => (
                common_directory(jobs.iter().map(|job| job.local_path.as_path())),
                common_directory(jobs.iter().map(|job| Path::new(job.remote_path.as_str())))
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        let failed: Vec<&&TransferJob> = jobs.iter().filter(|job| matches!(job.status, JobStatus::Failed(_))).collect();
        HistoryEntry {
            local_path: local_path.to_string_lossy().into_owned(),
            remote_path,
            files_done: jobs.iter().filter(|job| job.status == JobStatus::Completed).count(),
            files_total: jobs.len(),
            bytes: jobs
                .iter()
                .map(|job| if job.status == JobStatus::Completed { job.total_bytes } else { job.transferred_bytes })
                .sum(),
            failed_count: failed.len(),
            failed_files: failed.iter().take(MAX_FAILED_FILES).map(|job| job.display_name.clone()).collect(),
            ..new_entry((self.clock)(), connection, row.direction, row.label.clone(), history_result(row.state))
        }
    }

    pub(crate) fn record_failed_scan(&mut self, session_id: u64, direction: Direction, label: String, message: &str) {
        let connection = self.session_name(session_id);
        tracing::error!(target: "porthmos::transfers", connection = %connection, file = %label, "{message}");
        self.store_entry(new_entry((self.clock)(), connection, direction, label, HistoryResult::Failed));
        self.publish_history();
    }

    pub(crate) fn record_interrupted(&mut self) {
        self.process_row_changes();
        let rows = self.snapshot().rows;
        for row in rows.iter().filter(|row| !row.is_finished()) {
            if self.history.recorded.insert(row.kind) {
                let entry = self.entry_for_row(row);
                self.log_failed_jobs(row, &entry.connection);
                self.store_entry(entry);
            }
        }
    }

    fn log_failed_jobs(&self, row: &QueueRow, connection: &str) {
        for job in self.transfers.jobs_of(row.kind) {
            if let JobStatus::Failed(message) = &job.status {
                tracing::error!(target: "porthmos::transfers", connection = %connection, file = %job.display_name, "{message}");
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.writer.close();
        self.record_interrupted();
        self.tasks.abort_all();
    }
}

#[cfg(test)]
mod tests;
