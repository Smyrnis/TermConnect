use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::transfer::{conflicts::Resolution, plan::DirectoryPlan};

impl App {
    pub(super) fn review_or_apply_plan(&mut self, batch_id: u64, session_id: u64, direction: Direction, plan: DirectoryPlan) {
        let conflicts = transfer::conflicts::conflict_indices(&plan);
        if conflicts.is_empty() {
            self.apply_plan_ready(batch_id, session_id, direction, plan, &[]);
            return;
        }
        if let Some(resolution) = self.on_conflict.automatic_resolution() {
            let answers = vec![resolution; conflicts.len()];
            self.apply_plan_ready(batch_id, session_id, direction, plan, &answers);
            return;
        }
        self.conflict_reviews.push_back(ConflictReview { batch_id, session_id, direction, plan, conflicts, answers: Vec::new() });
        self.open_next_conflict_prompt();
    }

    pub(super) fn open_next_conflict_prompt(&mut self) {
        if self.dialog.is_some() || self.screen == Screen::Search {
            return;
        }
        let Some(review) = self.conflict_reviews.front() else {
            return;
        };
        let index = review.answers.len();
        let Some(file) = review.conflicts.get(index).and_then(|file_index| review.plan.files.get(*file_index)) else {
            return;
        };
        let Some(existing) = file.existing else {
            return;
        };
        let file_name = file.display_name.clone();
        self.dialog = Some(Dialog::Conflict(ConflictDialog { file_name, existing, new_size: file.size, new_modified: file.source_modified, index, total: review.conflicts.len(), apply_to_rest: false, now: unix_now() }));
        self.pending_action = Some(PendingAction::ResolveConflict);
    }

    pub(super) fn answer_conflict(&mut self, resolution: Option<Resolution>, apply_to_rest: bool) {
        let Some(mut review) = self.conflict_reviews.pop_front() else {
            return;
        };
        let Some(resolution) = resolution else {
            self.transfers.forget_batch_if_empty(review.batch_id);
            self.notifications.push(Severity::Info, "Copy cancelled");
            self.refresh_destination_panel(review.session_id, review.direction);
            self.open_next_conflict_prompt();
            return;
        };
        let remaining = review.conflicts.len() - review.answers.len();
        review.answers.extend(std::iter::repeat_n(resolution, if apply_to_rest { remaining } else { 1 }));
        if review.answers.len() < review.conflicts.len() {
            self.conflict_reviews.push_front(review);
        } else {
            self.apply_plan_ready(review.batch_id, review.session_id, review.direction, review.plan, &review.answers);
        }
        self.open_next_conflict_prompt();
    }

    pub(super) fn drop_conflict_reviews(&mut self, should_drop: impl Fn(&ConflictReview) -> bool) -> usize {
        let showing_dropped_review = matches!(self.pending_action, Some(PendingAction::ResolveConflict)) && self.conflict_reviews.front().is_some_and(&should_drop);
        let dropped: Vec<u64> = self.conflict_reviews.iter().filter(|review| should_drop(review)).map(|review| review.batch_id).collect();
        self.conflict_reviews.retain(|review| !should_drop(review));
        for batch_id in &dropped {
            self.transfers.forget_batch_if_empty(*batch_id);
        }
        if showing_dropped_review {
            self.dialog = None;
            self.pending_action = None;
        }
        self.open_next_conflict_prompt();
        dropped.len()
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/app/conflicts_test.rs"]
mod tests;
