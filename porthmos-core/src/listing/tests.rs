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

fn names(listing: &Listing) -> Vec<String> {
    listing
        .rows()
        .iter()
        .map(|row| match row {
            Row::Parent => "..".to_string(),
            Row::Entry(entry) => entry.name.clone(),
        })
        .collect()
}

fn filterable() -> Listing {
    Listing::new(
        PathBuf::from("/d"),
        vec![
            entry("porthmos", true, 0),
            entry("Report.pdf", false, 1),
            entry("notes.txt", false, 2),
            entry("app.log", false, 3),
        ],
        SortSpec::default(),
        false,
    )
}

#[test]
fn a_filter_keeps_names_containing_the_text_in_any_case() {
    let mut listing = filterable();

    listing.set_filter(Some("PORT"));

    assert_eq!(names(&listing), vec!["..", "porthmos", "Report.pdf"]);
    assert_eq!(listing.filter(), Some("PORT"));
}

#[test]
fn a_filter_with_wildcards_is_a_glob() {
    let mut listing = filterable();

    listing.set_filter(Some("*.LOG"));

    assert_eq!(names(&listing), vec!["..", "app.log"]);
}

#[test]
fn an_empty_filter_shows_everything() {
    let mut listing = filterable();
    listing.set_filter(Some("port"));

    listing.set_filter(Some(""));

    assert_eq!(names(&listing).len(), 5);
    assert_eq!(listing.filter(), None);
}

#[test]
fn the_filter_counts_matches_out_of_the_visible_entries() {
    let mut listing = filterable();
    listing.set_filter(Some("o"));

    assert_eq!(listing.match_count(), (4, 4));
    listing.set_filter(Some("pdf"));
    assert_eq!(listing.match_count(), (1, 4));
}

#[test]
fn a_new_folder_clears_the_filter_but_a_refresh_keeps_it() {
    let mut listing = filterable();
    listing.set_filter(Some("port"));

    listing.replace(PathBuf::from("/d"), vec![entry("porthmos", true, 0), entry("other", false, 1)]);
    assert_eq!(names(&listing), vec!["..", "porthmos"]);

    listing.replace(PathBuf::from("/d/porthmos"), vec![entry("x", false, 1)]);
    assert_eq!(listing.filter(), None);
    assert_eq!(names(&listing), vec!["..", "x"]);
}
