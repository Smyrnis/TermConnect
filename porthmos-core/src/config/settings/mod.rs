use serde::{Deserialize, Serialize};

use crate::{
    listing::{SortKey, SortOrder, SortSpec},
    transfer::conflicts::ConflictPolicy,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelSettings {
    pub show_hidden: bool,
    pub sort: SortSpec,
}

pub const MAX_PARALLEL_CAP: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSettings {
    pub max_parallel: usize,
    pub on_conflict: ConflictPolicy,
}

impl Default for TransferSettings {
    fn default() -> Self {
        Self { max_parallel: 4, on_conflict: ConflictPolicy::Ask }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub panel: PanelSettings,
    pub transfers: TransferSettings,
    pub frontend: toml::Table,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct SettingsFile {
    #[serde(default)]
    pub panel: PanelSettingsFile,
    #[serde(default)]
    pub transfers: TransferSettingsFile,
    #[serde(flatten)]
    pub frontend: toml::Table,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct TransferSettingsFile {
    pub max_parallel: Option<i64>,
    pub on_conflict: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct PanelSettingsFile {
    pub show_hidden: Option<bool>,
    pub sort_key: Option<String>,
    pub sort_order: Option<String>,
}

pub(crate) fn settings_from_file(file: SettingsFile) -> (Settings, Vec<String>) {
    let mut warnings = Vec::new();
    let show_hidden = file.panel.show_hidden.unwrap_or(false);

    let key = match file.panel.sort_key.as_deref() {
        None | Some("name") => SortKey::Name,
        Some("size") => SortKey::Size,
        Some(other) => {
            warnings.push(format!("unknown panel.sort_key \"{other}\", using \"name\""));
            SortKey::Name
        }
    };

    let order = match file.panel.sort_order.as_deref() {
        None | Some("ascending") => SortOrder::Ascending,
        Some("descending") => SortOrder::Descending,
        Some(other) => {
            warnings.push(format!("unknown panel.sort_order \"{other}\", using \"ascending\""));
            SortOrder::Ascending
        }
    };

    let max_parallel = match file.transfers.max_parallel {
        Some(value) if value > MAX_PARALLEL_CAP as i64 => {
            warnings.push(format!("transfers.max_parallel is capped at {MAX_PARALLEL_CAP}, using {MAX_PARALLEL_CAP}"));
            MAX_PARALLEL_CAP
        }
        Some(value) if value >= 1 => value as usize,
        Some(_) => {
            warnings.push("transfers.max_parallel must be at least 1, using 1".to_string());
            1
        }
        None => TransferSettings::default().max_parallel,
    };

    let on_conflict = match file.transfers.on_conflict.as_deref() {
        None | Some("ask") => ConflictPolicy::Ask,
        Some("overwrite") => ConflictPolicy::Overwrite,
        Some("skip") => ConflictPolicy::Skip,
        Some("rename") => ConflictPolicy::Rename,
        Some(other) => {
            warnings.push(format!("unknown transfers.on_conflict \"{other}\", using \"ask\""));
            ConflictPolicy::Ask
        }
    };

    (
        Settings {
            panel: PanelSettings { show_hidden, sort: SortSpec { key, order } },
            transfers: TransferSettings { max_parallel, on_conflict },
            frontend: file.frontend,
        },
        warnings,
    )
}

#[cfg(test)]
mod tests;
