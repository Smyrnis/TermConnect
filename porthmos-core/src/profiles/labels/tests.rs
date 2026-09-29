use super::*;

#[test]
fn a_group_is_trimmed_per_segment_and_empty_segments_are_dropped() {
    assert_eq!(normalize_group(" Clients / Acme/ "), Some("Clients/Acme".to_string()));
    assert_eq!(normalize_group("a//b"), Some("a/b".to_string()));
}

#[test]
fn a_group_of_only_slashes_or_spaces_is_none() {
    assert_eq!(normalize_group(" / "), None);
    assert_eq!(normalize_group(""), None);
}

#[test]
fn tags_are_trimmed_unhashed_and_deduplicated_by_case_keeping_order() {
    assert_eq!(normalize_tags([" prod", "#db", "", "Prod", "web "]), vec!["prod", "db", "web"]);
}

#[test]
fn parse_tags_splits_on_commas() {
    assert_eq!(parse_tags("prod, db,,#web"), vec!["prod", "db", "web"]);
}

#[test]
fn labels_are_empty_without_group_and_tags() {
    assert!(Labels::default().is_empty());
    assert!(!Labels { group: Some("a".into()), tags: Vec::new() }.is_empty());
}
