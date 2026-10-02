use std::sync::Arc;

use porthmos_core::sync::{SyncBy, SyncDirection, SyncOptions, SyncPlan};

use super::*;
use crate::widgets::{
    dialog::{FormDialog, FormField},
    filter_line::FilterLine,
};

const DIRECTION: &str = "direction";
const BY: &str = "by";
const SUBFOLDERS: &str = "subfolders";

fn choices(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(value, label)| (value.to_string(), label.to_string())).collect()
}

impl App {
    pub(super) fn open_sync(&mut self) {
        if self.screen != Screen::Files {
            return;
        }
        if self.sync_view.has_plan() {
            self.screen = Screen::Sync;
            return;
        }
        let Some(session) = self.sessions.active() else {
            self.notifications.push(Severity::Warning, "Connect to a remote server first");
            return;
        };
        if !session.listed {
            self.notifications.push(Severity::Warning, "Wait for the remote folder to finish loading");
            return;
        }
        let mut directions =
            vec![("local_to_remote", "Local \u{2192} Remote"), ("remote_to_local", "Remote \u{2192} Local")];
        if session.preserves_times {
            directions.push(("both", "Both (the newer file wins)"));
        }
        let fields = vec![
            FormField::choice(DIRECTION, "Direction", choices(&directions), "local_to_remote"),
            FormField::choice(BY, "Compare by", choices(&[("time", "Modification time"), ("size", "Size")]), "time"),
            FormField::choice(SUBFOLDERS, "Subfolders", choices(&[("yes", "Included"), ("no", "Not included")]), "yes"),
        ];
        let mut form = FormDialog::new(format!("Synchronize with {}", session.name), fields);
        if !session.preserves_times {
            form.hint = Some("Both isn't offered: this connection can't keep modification times".to_string());
        }
        let id = session.id;
        self.dialog = Some(Dialog::Form(form));
        self.pending_action = Some(PendingAction::StartSync { session: id });
    }

    pub(super) fn submit_sync_options(&mut self, session: SessionId, values: Vec<(&'static str, String)>) {
        let options = match parse_options(&values) {
            Ok(options) => options,
            Err(message) => {
                self.set_form_error(message);
                return;
            }
        };
        let remote_dir = self.sessions.by_id(session).map(|live| live.panel.path().to_path_buf());
        self.dialog = None;
        self.pending_action = None;
        let Some(remote_dir) = remote_dir else {
            self.notifications.push(Severity::Warning, "That connection is no longer available");
            return;
        };
        self.core.send(Command::StartSync { session, local_dir: self.local.path().to_path_buf(), remote_dir, options });
    }

    pub(super) fn apply_sync_plan(&mut self, plan: Arc<SyncPlan>) {
        if let Some(open_id) = self.sync_view.sync_id() {
            if open_id != plan.sync_id {
                self.core.send(Command::CancelSync { sync_id: plan.sync_id });
                self.notifications.push(Severity::Warning, "A sync preview is already open; the new one was dropped");
            }
            return;
        }
        self.sync_view.load(plan);
        let free_to_open =
            self.screen == Screen::Files && self.dialog.is_none() && !self.help_visible && !self.editing_filter();
        if free_to_open {
            self.screen = Screen::Sync;
        } else {
            self.notifications
                .push(Severity::Info, "Sync preview ready \u{2014} open it with the sync key (Ctrl+U by default)");
        }
    }

    pub(super) fn withdraw_sync(&mut self, sync_ids: &[u64]) {
        if self.sync_view.sync_id().is_some_and(|id| sync_ids.contains(&id)) {
            self.sync_view.clear();
            if self.screen == Screen::Sync {
                self.screen = Screen::Files;
            }
            self.notifications.push(Severity::Info, "The sync plan was cancelled");
        }
    }

    pub(super) fn apply_sync_action(&mut self, action: Action) {
        match action {
            Action::Up => self.sync_view.move_cursor(-1),
            Action::Down => self.sync_view.move_cursor(1),
            Action::ToggleSelect => self.sync_view.toggle(),
            Action::Open => self.run_sync_plan(),
            _ => {}
        }
    }

    pub(super) fn apply_sync_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('a') => self.sync_view.tick_all(),
            KeyCode::Char('n') => self.sync_view.untick_all(),
            KeyCode::Char('f') => self.sync_view.flip(),
            _ => {}
        }
    }

    fn run_sync_plan(&mut self) {
        let Some(sync_id) = self.sync_view.sync_id() else {
            return;
        };
        let choices = self.sync_view.visible_choices();
        if choices.is_empty() {
            let message = if self.sync_view.filter().is_some() {
                "Nothing is ticked in the rows shown"
            } else {
                "Nothing is ticked"
            };
            self.notifications.push(Severity::Warning, message);
            return;
        }
        self.core.send(Command::RunSync { sync_id, choices });
        self.sync_view.clear();
        self.screen = Screen::Files;
    }

    pub(super) fn leave_sync_screen(&mut self) {
        if let Some(sync_id) = self.sync_view.sync_id() {
            self.core.send(Command::CancelSync { sync_id });
        }
        self.sync_view.clear();
        self.screen = Screen::Files;
    }
}

fn unknown_choice(what: &str, value: &str) -> String {
    format!("Unknown {what} \"{value}\"")
}

fn parse_options(values: &[(&'static str, String)]) -> Result<SyncOptions, String> {
    let value =
        |key: &str| values.iter().find(|(name, _)| *name == key).map(|(_, text)| text.as_str()).unwrap_or_default();
    let direction = match value(DIRECTION) {
        "local_to_remote" => SyncDirection::LocalToRemote,
        "remote_to_local" => SyncDirection::RemoteToLocal,
        "both" => SyncDirection::Both,
        other => return Err(unknown_choice("direction", other)),
    };
    let by = match value(BY) {
        "time" => SyncBy::Time,
        "size" => SyncBy::Size,
        other => return Err(unknown_choice("comparison", other)),
    };
    let subfolders = match value(SUBFOLDERS) {
        "yes" => true,
        "no" => false,
        other => return Err(unknown_choice("subfolders option", other)),
    };
    let options = SyncOptions { direction, by, subfolders };
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

#[cfg(test)]
mod tests;
