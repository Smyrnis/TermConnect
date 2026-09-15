use std::cmp::Ordering;

use crate::filesystem::Entry;

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
        Self {
            key: SortKey::Name,
            order: SortOrder::Ascending,
        }
    }
}

impl SortSpec {
    /// Advances to the next spec in the cycle Name↑ → Name↓ → Size↑ →
    /// Size↓ → Name↑, used by `Ctrl+S`.
    pub fn cycled(self) -> SortSpec {
        match (self.key, self.order) {
            (SortKey::Name, SortOrder::Ascending) => SortSpec {
                key: SortKey::Name,
                order: SortOrder::Descending,
            },
            (SortKey::Name, SortOrder::Descending) => SortSpec {
                key: SortKey::Size,
                order: SortOrder::Ascending,
            },
            (SortKey::Size, SortOrder::Ascending) => SortSpec {
                key: SortKey::Size,
                order: SortOrder::Descending,
            },
            (SortKey::Size, SortOrder::Descending) => SortSpec {
                key: SortKey::Name,
                order: SortOrder::Ascending,
            },
        }
    }
}

/// Sorts `entries` in place. Directories always precede files, independent
/// of `spec`; within a group, ties always break ascending by lowercased
/// name so ordering stays deterministic under any key/order combination.
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
mod tests {
    use super::*;
    use crate::filesystem::Entry;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, size: u64) -> Entry {
        Entry {
            name: name.to_string(),
            path: PathBuf::from(format!("/{name}")),
            is_dir,
            size,
            permissions: None,
        }
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn directories_sort_before_files_regardless_of_key() {
        let mut entries = vec![entry("b_file.txt", false, 10), entry("a_dir", true, 0)];
        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Size,
                order: SortOrder::Descending,
            },
        );
        assert_eq!(names(&entries), vec!["a_dir", "b_file.txt"]);
    }

    #[test]
    fn name_ascending_is_case_insensitive() {
        let mut entries = vec![entry("Banana", false, 0), entry("apple", false, 0)];
        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Name,
                order: SortOrder::Ascending,
            },
        );
        assert_eq!(names(&entries), vec!["apple", "Banana"]);
    }

    #[test]
    fn name_descending_reverses_order() {
        let mut entries = vec![entry("apple", false, 0), entry("banana", false, 0)];
        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Name,
                order: SortOrder::Descending,
            },
        );
        assert_eq!(names(&entries), vec!["banana", "apple"]);
    }

    #[test]
    fn size_ascending_orders_by_byte_count() {
        let mut entries = vec![entry("big.txt", false, 100), entry("small.txt", false, 1)];
        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Size,
                order: SortOrder::Ascending,
            },
        );
        assert_eq!(names(&entries), vec!["small.txt", "big.txt"]);
    }

    #[test]
    fn equal_sizes_tiebreak_ascending_by_name() {
        let mut entries = vec![entry("b.txt", false, 5), entry("a.txt", false, 5)];
        sort_entries(
            &mut entries,
            SortSpec {
                key: SortKey::Size,
                order: SortOrder::Descending,
            },
        );
        assert_eq!(names(&entries), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn cycled_advances_name_asc_desc_size_asc_desc_then_wraps() {
        let start = SortSpec::default();
        assert_eq!(
            start,
            SortSpec {
                key: SortKey::Name,
                order: SortOrder::Ascending
            }
        );

        let s1 = start.cycled();
        assert_eq!(
            s1,
            SortSpec {
                key: SortKey::Name,
                order: SortOrder::Descending
            }
        );

        let s2 = s1.cycled();
        assert_eq!(
            s2,
            SortSpec {
                key: SortKey::Size,
                order: SortOrder::Ascending
            }
        );

        let s3 = s2.cycled();
        assert_eq!(
            s3,
            SortSpec {
                key: SortKey::Size,
                order: SortOrder::Descending
            }
        );

        assert_eq!(s3.cycled(), start);
    }
}
