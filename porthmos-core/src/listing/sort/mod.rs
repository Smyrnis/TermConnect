use std::cmp::Ordering;

use porthmos_vfs::Entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub key: SortKey,
    pub order: SortOrder,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self { key: SortKey::Name, order: SortOrder::Ascending }
    }
}

impl SortSpec {
    pub fn cycled(self) -> SortSpec {
        match (self.key, self.order) {
            (SortKey::Name, SortOrder::Ascending) => SortSpec { key: SortKey::Name, order: SortOrder::Descending },
            (SortKey::Name, SortOrder::Descending) => SortSpec { key: SortKey::Size, order: SortOrder::Ascending },
            (SortKey::Size, SortOrder::Ascending) => SortSpec { key: SortKey::Size, order: SortOrder::Descending },
            (SortKey::Size, SortOrder::Descending) => SortSpec { key: SortKey::Name, order: SortOrder::Ascending },
        }
    }
}

pub fn sort_entries(entries: &mut [Entry], spec: SortSpec) {
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| compare_by(a, b, spec)));
}

fn compare_by(a: &Entry, b: &Entry, spec: SortSpec) -> Ordering {
    let primary = match spec.key {
        SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        SortKey::Size => a.size.cmp(&b.size),
    };
    let primary = match spec.order {
        SortOrder::Ascending => primary,
        SortOrder::Descending => primary.reverse(),
    };
    primary.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
}

#[cfg(test)]
mod tests;
