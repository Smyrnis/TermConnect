use chrono::Local;
use porthmos_core::history::{HistoryEntry, HistoryResult};

use super::*;
use crate::widgets::{file_list::format_size, filter_line::FilterLine, history_view::printable};

const DETAIL_NAMES: usize = 10;

pub(super) fn details_title(entry: &HistoryEntry) -> String {
    format!("Transfer: {}", printable(&entry.label))
}

pub(super) fn details_text(entry: &HistoryEntry) -> String {
    let finished = entry.finished_at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S");
    let direction = match entry.direction {
        Direction::Upload => "Upload",
        Direction::Download => "Download",
    };
    let result = match &entry.result {
        HistoryResult::PartlyFailed { failed } => format!("partly failed ({failed} failed)"),
        other => other.text().to_string(),
    };
    let mut lines = vec![
        format!("Finished:   {finished}"),
        format!("Connection: {} ({direction})", printable(&entry.connection)),
        format!(
            "Result:     {result}, {} of {} files, {}",
            entry.files_done,
            entry.files_total,
            format_size(entry.bytes, false)
        ),
    ];
    if !entry.local_path.is_empty() {
        lines.push(format!("Local:      {}", printable(&entry.local_path)));
    }
    if !entry.remote_path.is_empty() {
        lines.push(format!("Remote:     {}", printable(&entry.remote_path)));
    }
    if !entry.failed_files.is_empty() {
        lines.push("Failed files:".to_string());
        lines.extend(entry.failed_files.iter().take(DETAIL_NAMES).map(|name| format!("- {}", printable(name))));
        let shown = entry.failed_files.len().min(DETAIL_NAMES);
        let hidden = entry.failed_count.max(entry.failed_files.len()).saturating_sub(shown);
        if hidden > 0 {
            lines.push(format!("and {hidden} more\u{2026}"));
        }
    }
    let failed = matches!(entry.result, HistoryResult::Failed | HistoryResult::PartlyFailed { .. });
    if failed || !entry.failed_files.is_empty() {
        lines.push("The reasons are in porthmos.log.".to_string());
    }
    lines.join("\n")
}

impl App {
    pub(super) fn open_history_screen(&mut self) {
        self.screen = Screen::History;
        self.history.go_to_top();
        self.core.send(Command::ListHistory);
    }

    pub(super) fn apply_history_action(&mut self, action: Action) {
        match action {
            Action::Up => self.history.move_cursor(-1),
            Action::Down => self.history.move_cursor(1),
            Action::Open => self.open_history_details(),
            _ => {}
        }
    }

    fn open_history_details(&mut self) {
        let Some(entry) = self.history.selected() else {
            return;
        };
        let dialog = MessageDialog::new(details_title(entry), details_text(entry));
        self.dialog = Some(Dialog::Message(dialog));
    }

    pub(super) fn open_clear_history_dialog(&mut self) {
        if self.history.total() == 0 {
            return;
        }
        self.dialog = Some(Dialog::Confirm(ConfirmDialog::new("Clear the whole transfer history?")));
        self.pending_action = Some(PendingAction::ClearHistory);
    }
}

#[cfg(test)]
mod tests;
