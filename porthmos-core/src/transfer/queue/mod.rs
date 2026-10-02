use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet},
    hash::Hash,
    ops::{Deref, DerefMut},
    path::PathBuf,
};

use super::{
    job::{Destination, Direction, JobStatus, TransferJob},
    rows::{QueueRow, RowKind, RowState},
};

const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchProgress {
    pub total_files: usize,
    pub completed_files: usize,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowChange {
    Finished(RowKind),
    Reopened(RowKind),
    Removed(RowKind),
}

#[derive(Debug, Clone)]
struct RowCounters {
    kind: RowKind,
    label: String,
    direction: Direction,
    order: Vec<u64>,
    queued: usize,
    running: usize,
    completed: usize,
    failed: usize,
    cancelled: usize,
    bytes_done: u64,
    bytes_total: u64,
}

impl RowCounters {
    fn new(kind: RowKind, label: String, direction: Direction) -> Self {
        Self {
            kind,
            label,
            direction,
            order: Vec::new(),
            queued: 0,
            running: 0,
            completed: 0,
            failed: 0,
            cancelled: 0,
            bytes_done: 0,
            bytes_total: 0,
        }
    }

    fn total(&self) -> usize {
        self.queued + self.running + self.completed + self.failed + self.cancelled
    }

    fn finished(&self) -> bool {
        self.total() > 0 && self.queued == 0 && self.running == 0
    }

    fn state(&self) -> RowState {
        if self.running > 0 {
            RowState::Running
        } else if self.queued > 0 {
            RowState::Queued
        } else if self.completed == self.total() {
            RowState::Done
        } else if self.failed == 0 {
            RowState::Cancelled
        } else if self.completed > 0 {
            RowState::PartlyFailed(self.failed)
        } else {
            RowState::Failed
        }
    }

    fn to_row(&self) -> QueueRow {
        QueueRow {
            kind: self.kind,
            label: self.label.clone(),
            direction: self.direction,
            files_done: self.completed,
            files_total: self.total(),
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total,
            state: self.state(),
        }
    }

    fn slot(&mut self, status: &JobStatus) -> &mut usize {
        match status {
            JobStatus::Queued => &mut self.queued,
            JobStatus::InProgress => &mut self.running,
            JobStatus::Completed => &mut self.completed,
            JobStatus::Failed(_) => &mut self.failed,
            JobStatus::Cancelled => &mut self.cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Shape {
    status: JobStatus,
    transferred: u64,
    session_id: u64,
    direction: Direction,
}

impl Shape {
    fn of(job: &TransferJob) -> Self {
        Self {
            status: job.status.clone(),
            transferred: job.transferred_bytes,
            session_id: job.session_id,
            direction: job.direction,
        }
    }
}

fn decrement<K: Hash + Eq>(map: &mut HashMap<K, usize>, key: K) {
    if let Some(count) = map.get_mut(&key) {
        *count -= 1;
        if *count == 0 {
            map.remove(&key);
        }
    }
}

#[derive(Default)]
pub struct TransferQueue {
    jobs: BTreeMap<u64, TransferJob>,
    batch_labels: HashMap<u64, String>,
    unkept_times_flagged: HashSet<u64>,
    next_id: u64,
    next_batch_id: u64,
    rows: BTreeMap<u64, RowCounters>,
    row_of: HashMap<u64, u64>,
    batch_row: HashMap<u64, u64>,
    running: BTreeSet<u64>,
    queued: BTreeSet<u64>,
    queued_by_session: HashMap<u64, BTreeSet<u64>>,
    pending: HashMap<(u64, Direction), usize>,
    changes: Vec<RowChange>,
}

pub struct JobGuard<'a> {
    queue: &'a mut TransferQueue,
    id: u64,
    before: Shape,
}

impl Deref for JobGuard<'_> {
    type Target = TransferJob;

    fn deref(&self) -> &TransferJob {
        &self.queue.jobs[&self.id]
    }
}

impl DerefMut for JobGuard<'_> {
    fn deref_mut(&mut self) -> &mut TransferJob {
        self.queue.jobs.get_mut(&self.id).expect("a guarded job stays in the queue")
    }
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.queue.reconcile(self.id, self.before.clone());
    }
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
        let label = match batch_id {
            Some(batch) => self.batch_labels.get(&batch).cloned().unwrap_or_else(|| display_name.clone()),
            None => display_name.clone(),
        };
        let job = TransferJob {
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
            resume: false,
            modified: None,
        };
        let shape = Shape::of(&job);
        let key = match batch_id {
            Some(batch) => *self.batch_row.entry(batch).or_insert(id),
            None => id,
        };
        let kind = match batch_id {
            Some(batch) => RowKind::Batch(batch),
            None => RowKind::Single(id),
        };
        let was_finished = self.rows.get(&key).is_some_and(RowCounters::finished);
        let counters = self.rows.entry(key).or_insert_with(|| RowCounters::new(kind, label, direction));
        counters.order.push(id);
        counters.bytes_total += total_bytes;
        self.row_of.insert(id, key);
        self.jobs.insert(id, job);
        self.account(id, &shape, true);
        if was_finished && !self.rows[&key].finished() {
            self.changes.push(RowChange::Reopened(kind));
        }
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
        if !self.batch_row.contains_key(&batch_id) {
            self.batch_labels.remove(&batch_id);
            self.unkept_times_flagged.remove(&batch_id);
        }
    }

    pub fn flag_unkept_times(&mut self, batch_id: u64) -> bool {
        self.unkept_times_flagged.insert(batch_id)
    }

    pub fn has_unkept_times_flag(&self, batch_id: u64) -> bool {
        self.unkept_times_flagged.contains(&batch_id)
    }

    pub fn jobs(&self) -> impl Iterator<Item = &TransferJob> {
        self.jobs.values()
    }

    pub fn rows(&self) -> Vec<QueueRow> {
        self.rows.values().map(RowCounters::to_row).collect()
    }

    fn row_key(&self, kind: RowKind) -> Option<u64> {
        let key = match kind {
            RowKind::Single(id) => self.rows.contains_key(&id).then_some(id),
            RowKind::Batch(batch) => self.batch_row.get(&batch).copied(),
            RowKind::Scan(_) => None,
        }?;
        (self.rows.get(&key)?.kind == kind).then_some(key)
    }

    pub fn row(&self, kind: RowKind) -> Option<QueueRow> {
        self.rows.get(&self.row_key(kind)?).map(RowCounters::to_row)
    }

    pub fn jobs_of(&self, kind: RowKind) -> Vec<&TransferJob> {
        let Some(counters) = self.row_key(kind).and_then(|key| self.rows.get(&key)) else {
            return Vec::new();
        };
        counters.order.iter().filter_map(|id| self.jobs.get(id)).collect()
    }

    pub fn take_changes(&mut self) -> Vec<RowChange> {
        std::mem::take(&mut self.changes)
    }

    pub fn get(&self, id: u64) -> Option<&TransferJob> {
        self.jobs.get(&id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<JobGuard<'_>> {
        let before = Shape::of(self.jobs.get(&id)?);
        Some(JobGuard { queue: self, id, before })
    }

    fn account(&mut self, id: u64, shape: &Shape, add: bool) {
        let Some(&key) = self.row_of.get(&id) else {
            return;
        };
        if let Some(counters) = self.rows.get_mut(&key) {
            let slot = counters.slot(&shape.status);
            *slot = if add { *slot + 1 } else { slot.saturating_sub(1) };
            counters.bytes_done = if add {
                counters.bytes_done + shape.transferred
            } else {
                counters.bytes_done.saturating_sub(shape.transferred)
            };
        }
        match shape.status {
            JobStatus::InProgress => {
                if add {
                    self.running.insert(id);
                } else {
                    self.running.remove(&id);
                }
            }
            JobStatus::Queued => {
                if add {
                    self.queued.insert(id);
                    self.queued_by_session.entry(shape.session_id).or_default().insert(id);
                } else {
                    self.queued.remove(&id);
                    if let Some(ids) = self.queued_by_session.get_mut(&shape.session_id) {
                        ids.remove(&id);
                        if ids.is_empty() {
                            self.queued_by_session.remove(&shape.session_id);
                        }
                    }
                }
            }
            _ => {}
        }
        if matches!(shape.status, JobStatus::Queued | JobStatus::InProgress) {
            let key = (shape.session_id, shape.direction);
            if add {
                *self.pending.entry(key).or_default() += 1;
            } else {
                decrement(&mut self.pending, key);
            }
        }
    }

    fn reconcile(&mut self, id: u64, before: Shape) {
        let Some(job) = self.jobs.get(&id) else {
            return;
        };
        let after = Shape::of(job);
        if after == before {
            return;
        }
        let Some(&key) = self.row_of.get(&id) else {
            return;
        };
        let was_finished = self.rows.get(&key).is_some_and(RowCounters::finished);
        self.account(id, &before, false);
        self.account(id, &after, true);
        if let Some(counters) = self.rows.get(&key) {
            match (was_finished, counters.finished()) {
                (false, true) => self.changes.push(RowChange::Finished(counters.kind)),
                (true, false) => self.changes.push(RowChange::Reopened(counters.kind)),
                _ => {}
            }
        }
    }

    fn update(&mut self, id: u64, change: impl FnOnce(&mut TransferJob)) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        let before = Shape::of(job);
        change(job);
        self.reconcile(id, before);
    }

    pub fn retry_jobs(&mut self, ids: &[u64]) -> usize {
        let mut count = 0;
        for id in ids {
            if self.jobs.get(id).is_some_and(|job| matches!(job.status, JobStatus::Failed(_) | JobStatus::Cancelled)) {
                self.update(*id, |job| {
                    job.resume = job.attempts > 0;
                    job.status = JobStatus::Queued;
                    job.attempts = 0;
                });
                count += 1;
            }
        }
        count
    }

    pub fn remove_jobs(&mut self, ids: &[u64]) {
        let mut touched_batches: HashSet<u64> = HashSet::new();
        let mut removed_by_row: BTreeMap<u64, HashSet<u64>> = BTreeMap::new();
        let mut was_finished: HashMap<RowKind, bool> = HashMap::new();
        for id in ids {
            let Some(job) = self.jobs.get(id) else {
                continue;
            };
            let shape = Shape::of(job);
            let total_bytes = job.total_bytes;
            let batch_id = job.batch_id;
            let Some(&key) = self.row_of.get(id) else {
                continue;
            };
            if let Some(counters) = self.rows.get(&key) {
                was_finished.entry(counters.kind).or_insert_with(|| counters.finished());
            }
            self.account(*id, &shape, false);
            if let Some(counters) = self.rows.get_mut(&key) {
                counters.bytes_total = counters.bytes_total.saturating_sub(total_bytes);
            }
            self.row_of.remove(id);
            self.jobs.remove(id);
            removed_by_row.entry(key).or_default().insert(*id);
            if let Some(batch) = batch_id {
                touched_batches.insert(batch);
            }
        }
        for (key, removed) in removed_by_row {
            let Some(counters) = self.rows.get_mut(&key) else {
                continue;
            };
            counters.order.retain(|id| !removed.contains(id));
            if counters.order.is_empty() {
                let kind = counters.kind;
                self.rows.remove(&key);
                if let RowKind::Batch(batch) = kind {
                    self.batch_row.remove(&batch);
                }
                was_finished.remove(&kind);
                self.changes.push(RowChange::Removed(kind));
            } else if removed.contains(&key) {
                self.rekey_row(key);
            }
        }
        for (kind, before) in was_finished {
            let now = self.row_key(kind).and_then(|key| self.rows.get(&key)).map(RowCounters::finished);
            match now {
                Some(true) if !before => self.changes.push(RowChange::Finished(kind)),
                Some(false) if before => self.changes.push(RowChange::Reopened(kind)),
                _ => {}
            }
        }
        for batch in touched_batches {
            self.forget_batch_if_empty(batch);
        }
    }

    fn rekey_row(&mut self, old_key: u64) {
        let Some(mut counters) = self.rows.remove(&old_key) else {
            return;
        };
        let Some(&new_key) = counters.order.first() else {
            return;
        };
        if let Some(first) = self.jobs.get(&new_key) {
            counters.direction = first.direction;
            if let RowKind::Batch(batch) = counters.kind
                && !self.batch_labels.contains_key(&batch)
            {
                counters.label = first.display_name.clone();
            }
        }
        for id in &counters.order {
            self.row_of.insert(*id, new_key);
        }
        if let RowKind::Batch(batch) = counters.kind {
            self.batch_row.insert(batch, new_key);
        }
        self.rows.insert(new_key, counters);
    }

    pub fn active_jobs(&self) -> impl Iterator<Item = &TransferJob> {
        self.running.iter().filter_map(|id| self.jobs.get(id))
    }

    pub fn queued_jobs(&self) -> impl Iterator<Item = &TransferJob> {
        self.queued.iter().filter_map(|id| self.jobs.get(id))
    }

    pub fn active_count(&self) -> usize {
        self.running.len()
    }

    pub fn startable(&self, limit: usize) -> Vec<u64> {
        self.startable_limited(limit, |_| None)
    }

    pub fn startable_limited(&self, limit: usize, session_limit: impl Fn(u64) -> Option<usize>) -> Vec<u64> {
        let free_slots = limit.saturating_sub(self.running.len());
        if free_slots == 0 || self.queued.is_empty() {
            return Vec::new();
        }
        let mut per_session: HashMap<u64, usize> = HashMap::new();
        for job in self.active_jobs() {
            *per_session.entry(job.session_id).or_default() += 1;
        }
        let capped = |session: u64, running: usize| session_limit(session).is_some_and(|cap| running >= cap);
        let mut iterators: HashMap<u64, std::collections::btree_set::Iter<'_, u64>> = HashMap::new();
        let mut heads: BinaryHeap<Reverse<(u64, u64)>> = BinaryHeap::new();
        for (session, ids) in &self.queued_by_session {
            if capped(*session, per_session.get(session).copied().unwrap_or(0)) {
                continue;
            }
            let mut iterator = ids.iter();
            if let Some(&first) = iterator.next() {
                heads.push(Reverse((first, *session)));
                iterators.insert(*session, iterator);
            }
        }
        let mut busy_destinations: HashSet<Destination> = self.active_jobs().map(TransferJob::destination).collect();
        let mut startable = Vec::new();
        while startable.len() < free_slots {
            let Some(Reverse((id, session))) = heads.pop() else {
                break;
            };
            if capped(session, per_session.get(&session).copied().unwrap_or(0)) {
                continue;
            }
            if let Some(&next) = iterators.get_mut(&session).and_then(Iterator::next) {
                heads.push(Reverse((next, session)));
            }
            let Some(job) = self.jobs.get(&id) else {
                continue;
            };
            if busy_destinations.insert(job.destination()) {
                *per_session.entry(session).or_default() += 1;
                startable.push(job.id);
            }
        }
        startable
    }

    pub fn active_ids_for_session(&self, session_id: u64) -> Vec<u64> {
        self.active_jobs().filter(|job| job.session_id == session_id).map(|job| job.id).collect()
    }

    pub fn has_pending(&self, session_id: u64, direction: Direction) -> bool {
        self.pending.get(&(session_id, direction)).copied().unwrap_or(0) > 0
    }

    pub fn cancel_all_queued(&mut self) -> usize {
        let ids: Vec<u64> = self.queued.iter().copied().collect();
        for id in &ids {
            self.update(*id, |job| job.status = JobStatus::Cancelled);
        }
        ids.len()
    }

    pub fn queued_count(&self) -> usize {
        self.queued.len()
    }

    pub fn fail_queued_for_session(&mut self, session_id: u64, reason: &str) -> usize {
        let ids: Vec<u64> =
            self.queued_by_session.get(&session_id).map(|ids| ids.iter().copied().collect()).unwrap_or_default();
        for id in &ids {
            self.update(*id, |job| job.status = JobStatus::Failed(reason.to_string()));
        }
        ids.len()
    }

    pub fn retry_or_give_up(&mut self, id: u64) -> bool {
        let Some(job) = self.jobs.get(&id) else {
            return false;
        };
        if job.attempts < MAX_ATTEMPTS {
            self.update(id, |job| {
                job.status = JobStatus::Queued;
                job.resume = true;
            });
            true
        } else {
            false
        }
    }

    pub fn batch_progress(&self, batch_id: u64) -> BatchProgress {
        let counters = self.batch_row.get(&batch_id).and_then(|key| self.rows.get(key));
        match counters {
            Some(counters) => BatchProgress {
                total_files: counters.total(),
                completed_files: counters.completed,
                total_bytes: counters.bytes_total,
                transferred_bytes: counters.bytes_done,
            },
            None => BatchProgress { total_files: 0, completed_files: 0, total_bytes: 0, transferred_bytes: 0 },
        }
    }

    #[cfg(test)]
    pub(crate) fn corrupt_for_test(&mut self) {
        if let Some(counters) = self.rows.values_mut().next() {
            counters.completed += 1;
        }
    }

    #[cfg(test)]
    pub(crate) fn assert_consistent(&self) {
        let mut running = BTreeSet::new();
        let mut queued = BTreeSet::new();
        let mut queued_by_session: HashMap<u64, BTreeSet<u64>> = HashMap::new();
        let mut pending: HashMap<(u64, Direction), usize> = HashMap::new();
        for job in self.jobs.values() {
            match job.status {
                JobStatus::InProgress => {
                    running.insert(job.id);
                }
                JobStatus::Queued => {
                    queued.insert(job.id);
                    queued_by_session.entry(job.session_id).or_default().insert(job.id);
                }
                _ => {}
            }
            if matches!(job.status, JobStatus::Queued | JobStatus::InProgress) {
                *pending.entry((job.session_id, job.direction)).or_default() += 1;
            }
        }
        assert_eq!(self.running, running, "running set");
        assert_eq!(self.queued, queued, "queued set");
        assert_eq!(self.queued_by_session, queued_by_session, "queued per session");
        assert_eq!(self.pending, pending, "pending counts");
        assert_eq!(self.row_of.len(), self.jobs.len(), "row_of covers every job");
        let mut in_rows = 0;
        for (key, counters) in &self.rows {
            assert!(!counters.order.is_empty(), "an empty row {key} remains");
            assert_eq!(counters.order.first(), Some(key), "row key is its first job");
            let mut seen = HashSet::new();
            let mut expected = RowCounters::new(counters.kind, counters.label.clone(), counters.direction);
            for id in &counters.order {
                assert!(seen.insert(*id), "job {id} listed twice in row {key}");
                let job = self.jobs.get(id).expect("every listed job exists");
                assert_eq!(self.row_of.get(id), Some(key), "row_of for job {id}");
                *expected.slot(&job.status) += 1;
                expected.bytes_done += job.transferred_bytes;
                expected.bytes_total += job.total_bytes;
                in_rows += 1;
            }
            assert_eq!(
                (counters.queued, counters.running, counters.completed, counters.failed, counters.cancelled),
                (expected.queued, expected.running, expected.completed, expected.failed, expected.cancelled),
                "row counters differ for row {key}"
            );
            assert_eq!(
                (counters.bytes_done, counters.bytes_total),
                (expected.bytes_done, expected.bytes_total),
                "row bytes differ for row {key}"
            );
            match counters.kind {
                RowKind::Batch(batch) => assert_eq!(self.batch_row.get(&batch), Some(key), "batch_row for {batch}"),
                RowKind::Single(id) => assert_eq!(id, *key, "a single row is keyed by its job"),
                RowKind::Scan(_) => panic!("a scan row in the queue"),
            }
        }
        assert_eq!(in_rows, self.jobs.len(), "rows cover every job");
        for (batch, key) in &self.batch_row {
            let kind = self.rows.get(key).map(|counters| counters.kind);
            assert_eq!(kind, Some(RowKind::Batch(*batch)), "batch_row entry for {batch}");
        }
    }
}

#[cfg(test)]
mod tests;
