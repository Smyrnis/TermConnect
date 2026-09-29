use super::super::*;
use crate::widgets::dialog::message::MessageDialog;

fn unannounced(entries: &[ConnectionEntry], source: ConnectionSource, announced: &HashSet<String>) -> Vec<String> {
    let mut names: Vec<String> = entries
        .iter()
        .filter(|entry| entry.source == source && !announced.contains(&entry.name))
        .map(|entry| entry.name.clone())
        .collect();
    names.sort();
    names
}

impl App {
    pub(in crate::app) fn announce_missing_hosts(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        let entries = self.connections.entries();
        let missing = unannounced(entries, ConnectionSource::MissingSshHost, &self.announced_missing);
        let shadowed = unannounced(entries, ConnectionSource::ShadowedSshHost, &self.announced_missing);
        if missing.is_empty() && shadowed.is_empty() {
            return;
        }
        let mut sentences = Vec::new();
        if !missing.is_empty() {
            sentences.push(format!("These labelled hosts are no longer in ~/.ssh/config: {}.", missing.join(", ")));
        }
        if !shadowed.is_empty() {
            sentences.push(format!(
                "These labelled hosts are hidden by a saved connection of the same name: {}.",
                shadowed.join(", ")
            ));
        }
        sentences.push("They are marked \u{26A0} in the connection list.".to_string());
        self.announced_missing.extend(missing.into_iter().chain(shadowed));
        self.dialog = Some(Dialog::Message(MessageDialog::new("Labelled ssh hosts", sentences.join(" "))));
    }

    pub(in crate::app) fn open_missing_host_dialog(&mut self) {
        let Some(entry) = self.connections.selected_entry() else {
            return;
        };
        let name = entry.name.clone();
        let title = if entry.source == ConnectionSource::ShadowedSshHost {
            format!("{name} is hidden by a saved connection")
        } else {
            format!("{name} is no longer in ~/.ssh/config")
        };
        let items = vec!["Move labels to\u{2026}".to_string(), "Forget labels".to_string(), "Cancel".to_string()];
        self.dialog = Some(Dialog::List(ListDialog::new(title, items)));
        self.pending_action = Some(PendingAction::FixMissingHost { name });
    }

    pub(in crate::app) fn apply_missing_host_choice(&mut self, name: String, index: usize) {
        self.dialog = None;
        match index {
            0 => {
                let candidates: Vec<String> = self
                    .connections
                    .entries()
                    .iter()
                    .filter(|entry| entry.source == ConnectionSource::SshConfig)
                    .filter(|entry| entry.group.is_none() && entry.tags.is_empty())
                    .map(|entry| entry.name.clone())
                    .collect();
                if candidates.is_empty() {
                    self.notifications.push(Severity::Warning, "No unlabelled ssh hosts to move the labels to");
                    return;
                }
                self.dialog =
                    Some(Dialog::List(ListDialog::new(format!("Move the labels of {name} to"), candidates.clone())));
                self.pending_action = Some(PendingAction::MoveLabels { from: name, candidates });
            }
            1 => self.core.send(Command::ForgetSshLabels { name }),
            _ => {}
        }
    }

    pub(in crate::app) fn move_labels_to(&mut self, from: String, candidates: Vec<String>, index: usize) {
        self.dialog = None;
        if let Some(to) = candidates.into_iter().nth(index) {
            self.core.send(Command::MoveSshLabels { from, to });
        }
    }
}

#[cfg(test)]
mod tests;
