use chrono::{TimeZone, Utc};

use super::{HistoryEntry, HistoryResult};
use crate::transfer::Direction;

pub(crate) fn sample(label: &str, result: HistoryResult) -> HistoryEntry {
    HistoryEntry {
        finished_at: Utc.with_ymd_and_hms(2026, 9, 30, 8, 12, 44).unwrap(),
        connection: "prod".to_string(),
        direction: Direction::Upload,
        label: label.to_string(),
        local_path: format!("/home/me/{label}"),
        remote_path: format!("/srv/{label}"),
        files_done: 1,
        files_total: 1,
        bytes: 42,
        result,
        failed_count: 0,
        failed_files: Vec::new(),
    }
}

pub(crate) trait SaveNow {
    fn record(&mut self, entry: HistoryEntry) -> anyhow::Result<()>;
    fn clear(&mut self) -> anyhow::Result<()>;
    fn save_now(&self) -> anyhow::Result<()>;
}

impl SaveNow for super::History {
    fn record(&mut self, entry: HistoryEntry) -> anyhow::Result<()> {
        self.push(entry);
        self.save_now()
    }

    fn clear(&mut self) -> anyhow::Result<()> {
        self.clear_memory();
        self.save_now()
    }

    fn save_now(&self) -> anyhow::Result<()> {
        if !self.is_writable() {
            anyhow::bail!("the existing history file could not be read or set aside, so it was left alone");
        }
        crate::persist::write_atomic(self.path(), self.render()?.as_bytes(), 0o600)
    }
}
