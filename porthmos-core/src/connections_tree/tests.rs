use std::collections::HashSet;

use super::*;
use crate::profiles::{ConnectionEntry, ConnectionSource, Labels};

fn entry(name: &str, group: Option<&str>, tags: &[&str]) -> ConnectionEntry {
    let mut entry = ConnectionEntry::orphan_labels(
        name.to_string(),
        Labels {
            group: group.map(str::to_string),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            in_keyring: Vec::new(),
        },
        ConnectionSource::MissingSshHost,
    );
    entry.host = format!("{name}.example");
    entry
}

fn open(paths: &[&str]) -> HashSet<String> {
    paths.iter().map(|path| path.to_string()).collect()
}

fn shape(entries: &[ConnectionEntry], rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            TreeRow::Group { name, depth, count, expanded, .. } => {
                format!("{}{}{name}({count})", "  ".repeat(*depth), if *expanded { "v" } else { ">" })
            }
            TreeRow::Connection { index, depth } => format!("{}{}", "  ".repeat(*depth), entries[*index].name),
        })
        .collect()
}

fn sample() -> Vec<ConnectionEntry> {
    vec![
        entry("alpha", Some("Work/Web"), &["prod"]),
        entry("beta", None, &[]),
        entry("gamma", Some("Work"), &["db"]),
        entry("delta", Some("Home"), &[]),
    ]
}

#[test]
fn everything_starts_collapsed_with_groups_before_ungrouped() {
    let entries = sample();
    assert_eq!(shape(&entries, &rows(&entries, &HashSet::new(), None)), vec![">Home(1)", ">Work(2)", "beta"]);
}

#[test]
fn expanding_shows_subgroups_first_then_connections() {
    let entries = sample();
    let rows = rows(&entries, &open(&["Work", "Work/Web"]), None);
    assert_eq!(shape(&entries, &rows), vec![">Home(1)", "vWork(2)", "  vWeb(1)", "    alpha", "  gamma", "beta"]);
}

#[test]
fn a_collapsed_parent_hides_an_expanded_child() {
    let entries = sample();
    let rows = rows(&entries, &open(&["Work/Web"]), None);
    assert_eq!(shape(&entries, &rows), vec![">Home(1)", ">Work(2)", "beta"]);
}

#[test]
fn groups_differing_by_case_are_separate() {
    let entries = vec![entry("a", Some("work"), &[]), entry("b", Some("Work"), &[])];
    assert_eq!(shape(&entries, &rows(&entries, &HashSet::new(), None)), vec![">Work(1)", ">work(1)"]);
}

#[test]
fn deep_nesting_counts_every_level() {
    let entries = vec![entry("a", Some("1/2/3/4/5"), &[]), entry("b", Some("1/2"), &[])];
    let rows = rows(&entries, &open(&["1", "1/2", "1/2/3", "1/2/3/4", "1/2/3/4/5"]), None);
    assert_eq!(
        shape(&entries, &rows),
        vec!["v1(2)", "  v2(2)", "    v3(1)", "      v4(1)", "        v5(1)", "          a", "    b"]
    );
}

#[test]
fn a_filter_opens_matching_groups_and_drops_the_rest() {
    let entries = sample();
    let rows = rows(&entries, &HashSet::new(), Some("alp"));
    assert_eq!(shape(&entries, &rows), vec!["vWork(1)", "  vWeb(1)", "    alpha"]);
}

#[test]
fn the_filter_matches_host_group_and_tag_case_insensitively() {
    let entries = sample();
    assert!(matches(&entries[0], "ALPHA.EXAMPLE"));
    assert!(matches(&entries[0], "work/w"));
    assert!(matches(&entries[0], "PRO"));
    assert!(!matches(&entries[1], "prod"));
}

#[test]
fn a_hash_filter_matches_whole_tags_only() {
    let mut entries = sample();
    entries.push(entry("prod-box", None, &["production"]));
    assert!(matches(&entries[0], "#PROD"));
    assert!(!matches(&entries[4], "#prod"));
    assert!(!matches(&entries[0], "#alpha"));
}

#[test]
fn a_bare_hash_filter_matches_everything() {
    let entries = sample();
    assert_eq!(match_count(&entries, Some("# ")), 4);
    assert_eq!(rows(&entries, &HashSet::new(), Some("#")), rows(&entries, &HashSet::new(), None));
}

#[test]
fn match_count_counts_matching_connections() {
    let entries = sample();
    assert_eq!(match_count(&entries, None), 4);
    assert_eq!(match_count(&entries, Some("a")), 4);
    assert_eq!(match_count(&entries, Some("#db")), 1);
}

#[test]
fn only_filters_with_something_to_match_are_active() {
    assert_eq!(active_filter(Some(" # ")), None);
    assert_eq!(active_filter(Some("")), None);
    assert_eq!(active_filter(None), None);
    assert_eq!(active_filter(Some("#db")), Some("#db"));
}
