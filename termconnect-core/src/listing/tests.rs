use std::path::{Path, PathBuf};

use super::*;

fn entry(name: &str, is_dir: bool, size: u64) -> Entry {
    Entry { name: name.to_string(), path: PathBuf::from("/d").join(name), is_dir, size, permissions: None }
}

#[test]
fn a_listing_below_the_root_starts_with_a_parent_row() {
    let listing = Listing::new(PathBuf::from("/d"), vec![entry("a", false, 1)], SortSpec::default(), false);

    assert_eq!(listing.rows()[0], Row::Parent);
    assert_eq!(listing.rows().len(), 2);
}

#[test]
fn a_listing_at_the_root_has_no_parent_row() {
    let listing = Listing::new(PathBuf::from("/"), Vec::new(), SortSpec::default(), false);

    assert!(listing.rows().is_empty());
}

#[test]
fn hidden_entries_are_filtered_until_shown() {
    let mut listing = Listing::new(
        PathBuf::from("/d"),
        vec![entry(".secret", false, 1), entry("open", false, 1)],
        SortSpec::default(),
        false,
    );

    assert_eq!(listing.rows().len(), 2);
    listing.toggle_hidden();
    assert_eq!(listing.rows().len(), 3);
    assert!(listing.show_hidden());
}

#[test]
fn directories_sort_before_files_and_the_sort_can_cycle() {
    let mut listing = Listing::new(
        PathBuf::from("/d"),
        vec![entry("b.txt", false, 5), entry("zdir", true, 0), entry("a.txt", false, 9)],
        SortSpec::default(),
        false,
    );
    let names = |listing: &Listing| -> Vec<String> {
        listing
            .rows()
            .iter()
            .filter_map(|row| match row {
                Row::Entry(entry) => Some(entry.name.clone()),
                Row::Parent => None,
            })
            .collect()
    };

    assert_eq!(names(&listing), ["zdir", "a.txt", "b.txt"]);
    listing.cycle_sort();
    assert_eq!(listing.sort_spec(), SortSpec { key: SortKey::Name, order: SortOrder::Descending });
    assert_eq!(names(&listing), ["zdir", "b.txt", "a.txt"]);
}

#[test]
fn replacing_keeps_the_view_settings() {
    let mut listing = Listing::new(PathBuf::from("/d"), Vec::new(), SortSpec::default(), true);

    listing.replace(PathBuf::from("/e"), vec![entry(".x", false, 1)]);

    assert_eq!(listing.path(), Path::new("/e"));
    assert_eq!(listing.rows().len(), 2);
}
