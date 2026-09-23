use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::transfer::{conflicts::Resolution, plan::DirectoryPlan};

impl App {
    pub(super) fn review_or_apply_plan(
        &mut self, batch_id: u64, session_id: u64, direction: Direction, plan: DirectoryPlan,
    ) {
        let conflicts = transfer::conflicts::conflict_indices(&plan);
        if conflicts.is_empty() {
            self.apply_plan_ready(batch_id, session_id, direction, plan, &[]);
            return;
        }
        let automatic: Option<Vec<Resolution>> =
            conflicts.iter().map(|index| self.on_conflict.resolution_for(&plan.files[*index])).collect();
        if let Some(answers) = automatic {
            self.apply_plan_ready(batch_id, session_id, direction, plan, &answers);
            return;
        }
        let answers = vec![None; conflicts.len()];
        self.conflict_reviews.push_back(ConflictReview { batch_id, session_id, direction, plan, conflicts, answers });
        self.open_next_conflict_prompt();
    }

    pub(super) fn open_next_conflict_prompt(&mut self) {
        if self.dialog.is_some() || self.screen == Screen::Search {
            return;
        }
        let Some(review) = self.conflict_reviews.front() else {
            return;
        };
        let Some(index) = review.answers.iter().position(Option::is_none) else {
            return;
        };
        let Some(file) = review.conflicts.get(index).and_then(|file_index| review.plan.files.get(*file_index)) else {
            return;
        };
        if !file.is_conflict() {
            return;
        }
        let file_name = file.display_name.clone();
        self.dialog = Some(Dialog::Conflict(ConflictDialog {
            file_name,
            existing: file.existing,
            partial: file.partial,
            new_size: file.size,
            new_modified: file.source_modified,
            index,
            total: review.conflicts.len(),
            apply_to_rest: false,
            now: unix_now(),
        }));
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
        let Some(current) = review.answers.iter().position(Option::is_none) else {
            return;
        };
        review.answers[current] = Some(resolution);
        if apply_to_rest {
            let answered = &review.plan.files[review.conflicts[current]];
            for index in current + 1..review.conflicts.len() {
                if review.answers[index].is_none()
                    && transfer::conflicts::fits_the_rest(
                        resolution,
                        answered,
                        &review.plan.files[review.conflicts[index]],
                    )
                {
                    review.answers[index] = Some(resolution);
                }
            }
        }
        match review.answers.iter().copied().collect::<Option<Vec<Resolution>>>() {
            Some(answers) => {
                self.apply_plan_ready(review.batch_id, review.session_id, review.direction, review.plan, &answers)
            }
            None => self.conflict_reviews.push_front(review),
        }
        self.open_next_conflict_prompt();
    }

    pub(super) fn drop_conflict_reviews(&mut self, should_drop: impl Fn(&ConflictReview) -> bool) -> usize {
        let showing_dropped_review = matches!(self.pending_action, Some(PendingAction::ResolveConflict))
            && self.conflict_reviews.front().is_some_and(&should_drop);
        let dropped: Vec<u64> =
            self.conflict_reviews.iter().filter(|review| should_drop(review)).map(|review| review.batch_id).collect();
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
