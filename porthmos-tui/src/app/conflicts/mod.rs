use std::time::{SystemTime, UNIX_EPOCH};

use porthmos_core::transfer::conflicts::fits_the_rest;

use super::*;

impl App {
    pub(super) fn queue_conflict_prompt(&mut self, batch_id: u64, files: Vec<ConflictInfo>) {
        let answers = vec![None; files.len()];
        self.conflict_prompts.push_back(ConflictPrompt { batch_id, files, answers });
        self.open_next_conflict_prompt();
    }

    pub(super) fn open_next_conflict_prompt(&mut self) {
        if self.dialog.is_some() || self.screen == Screen::Search {
            return;
        }
        let Some(prompt) = self.conflict_prompts.front() else {
            return;
        };
        let Some(index) = prompt.answers.iter().position(Option::is_none) else {
            return;
        };
        let Some(file) = prompt.files.get(index) else {
            return;
        };
        self.dialog = Some(Dialog::Conflict(ConflictDialog {
            file_name: file.display_name.clone(),
            existing: file.existing,
            partial: file.partial,
            new_size: file.new_size,
            new_modified: file.new_modified,
            index,
            total: prompt.files.len(),
            apply_to_rest: false,
            now: unix_now(),
        }));
        self.pending_action = Some(PendingAction::ResolveConflict);
    }

    pub(super) fn answer_conflict(&mut self, resolution: Option<Resolution>, apply_to_rest: bool) {
        let Some(mut prompt) = self.conflict_prompts.pop_front() else {
            return;
        };
        let Some(resolution) = resolution else {
            self.core.send(Command::ResolveConflicts { batch_id: prompt.batch_id, answers: None });
            self.open_next_conflict_prompt();
            return;
        };
        let Some(current) = prompt.answers.iter().position(Option::is_none) else {
            return;
        };
        prompt.answers[current] = Some(resolution);
        if apply_to_rest {
            let answered = &prompt.files[current];
            for index in current + 1..prompt.files.len() {
                if prompt.answers[index].is_none() && fits_the_rest(resolution, answered, &prompt.files[index]) {
                    prompt.answers[index] = Some(resolution);
                }
            }
        }
        match prompt.answers.iter().copied().collect::<Option<Vec<Resolution>>>() {
            Some(answers) => {
                self.core.send(Command::ResolveConflicts { batch_id: prompt.batch_id, answers: Some(answers) })
            }
            None => self.conflict_prompts.push_front(prompt),
        }
        self.open_next_conflict_prompt();
    }

    pub(super) fn drop_conflict_prompts(&mut self, batch_ids: &[u64]) {
        let showing_dropped_prompt = matches!(self.pending_action, Some(PendingAction::ResolveConflict))
            && self.conflict_prompts.front().is_some_and(|prompt| batch_ids.contains(&prompt.batch_id));
        self.conflict_prompts.retain(|prompt| !batch_ids.contains(&prompt.batch_id));
        if showing_dropped_prompt {
            self.dialog = None;
            self.pending_action = None;
        }
        self.open_next_conflict_prompt();
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests;
