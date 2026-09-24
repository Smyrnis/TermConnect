use std::path::PathBuf;

use super::*;
use termconnect_vfs::Entry;

fn entry(name: &str, is_dir: bool, size: u64) -> Entry {
    Entry { name: name.to_string(), path: PathBuf::from(format!("/{name}")), is_dir, size, permissions: None }
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn directories_sort_before_files_regardless_of_key() {
    let mut entries = vec![entry("b_file.txt", false, 10), entry("a_dir", true, 0)];
    sort_entries(&mut entries, SortSpec { key: SortKey::Size, order: SortOrder::Descending });
    assert_eq!(names(&entries), vec!["a_dir", "b_file.txt"]);
}

#[test]
fn name_ascending_is_case_insensitive() {
    let mut entries = vec![entry("Banana", false, 0), entry("apple", false, 0)];
    sort_entries(&mut entries, SortSpec { key: SortKey::Name, order: SortOrder::Ascending });
    assert_eq!(names(&entries), vec!["apple", "Banana"]);
}

#[test]
fn name_descending_reverses_order() {
    let mut entries = vec![entry("apple", false, 0), entry("banana", false, 0)];
    sort_entries(&mut entries, SortSpec { key: SortKey::Name, order: SortOrder::Descending });
    assert_eq!(names(&entries), vec!["banana", "apple"]);
}

#[test]
fn size_ascending_orders_by_byte_count() {
    let mut entries = vec![entry("big.txt", false, 100), entry("small.txt", false, 1)];
    sort_entries(&mut entries, SortSpec { key: SortKey::Size, order: SortOrder::Ascending });
    assert_eq!(names(&entries), vec!["small.txt", "big.txt"]);
}

#[test]
fn equal_sizes_tiebreak_ascending_by_name() {
    let mut entries = vec![entry("b.txt", false, 5), entry("a.txt", false, 5)];
    sort_entries(&mut entries, SortSpec { key: SortKey::Size, order: SortOrder::Descending });
    assert_eq!(names(&entries), vec!["a.txt", "b.txt"]);
}

#[test]
fn cycled_advances_name_asc_desc_size_asc_desc_then_wraps() {
    let start = SortSpec::default();
    assert_eq!(start, SortSpec { key: SortKey::Name, order: SortOrder::Ascending });

    let s1 = start.cycled();
    assert_eq!(s1, SortSpec { key: SortKey::Name, order: SortOrder::Descending });

    let s2 = s1.cycled();
    assert_eq!(s2, SortSpec { key: SortKey::Size, order: SortOrder::Ascending });

    let s3 = s2.cycled();
    assert_eq!(s3, SortSpec { key: SortKey::Size, order: SortOrder::Descending });

    assert_eq!(s3.cycled(), start);
}
