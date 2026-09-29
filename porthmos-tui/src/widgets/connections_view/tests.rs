use porthmos_core::profiles::{ConnectionEntry, ConnectionSource, Labels};

use super::*;
use crate::widgets::filter_line::FilterLine;

fn entry(name: &str, group: Option<&str>) -> ConnectionEntry {
    let mut entry = ConnectionEntry::orphan_labels(
        name.to_string(),
        Labels { group: group.map(str::to_string), tags: Vec::new() },
        ConnectionSource::MissingSshHost,
    );
    entry.source = ConnectionSource::Profile;
    entry
}

fn view() -> ConnectionsView {
    let mut view = ConnectionsView::new();
    view.replace(vec![entry("alpha", Some("Work/Web")), entry("beta", None), entry("gamma", Some("Work"))]);
    view
}

fn selected_name(view: &ConnectionsView) -> String {
    match view.selected() {
        Some(Selection::Group(path)) => format!("group:{path}"),
        Some(Selection::Connection(entry)) => entry.name.clone(),
        None => "none".into(),
    }
}

#[test]
fn it_starts_collapsed_on_the_first_row() {
    let view = view();
    assert_eq!(view.rows().len(), 2);
    assert_eq!(selected_name(&view), "group:Work");
}

#[test]
fn toggle_and_expand_open_a_group_and_collapse_or_parent_climbs() {
    let mut view = view();
    view.toggle();
    assert_eq!(view.rows().len(), 4);
    view.move_cursor(1);
    assert_eq!(selected_name(&view), "group:Work/Web");
    view.expand();
    view.move_cursor(1);
    assert_eq!(selected_name(&view), "alpha");

    view.collapse_or_parent();
    assert_eq!(selected_name(&view), "group:Work/Web");
    view.collapse_or_parent();
    assert_eq!(view.rows().len(), 4);
    view.collapse_or_parent();
    assert_eq!(selected_name(&view), "group:Work");
}

#[test]
fn a_reload_keeps_the_cursor_on_the_same_item() {
    let mut view = view();
    view.move_cursor(1);
    assert_eq!(selected_name(&view), "beta");

    view.replace(vec![
        entry("aaa", None),
        entry("alpha", Some("Work/Web")),
        entry("beta", None),
        entry("gamma", Some("Work")),
    ]);

    assert_eq!(selected_name(&view), "beta");
}

#[test]
fn a_reload_that_removes_the_group_under_the_cursor_clamps() {
    let mut view = view();
    view.replace(vec![entry("alpha", Some("Solo"))]);
    view.cursor = 0;
    view.replace(vec![entry("alpha", None)]);
    assert_eq!(selected_name(&view), "alpha");
    view.replace(Vec::new());
    assert!(view.selected().is_none());
}

#[test]
fn reveal_opens_the_groups_and_lands_on_the_connection() {
    let mut view = view();
    view.reveal("alpha");
    assert_eq!(selected_name(&view), "alpha");
}

#[test]
fn the_filter_narrows_and_keeps_the_item_when_it_still_matches() {
    let mut view = view();
    view.reveal("gamma");
    view.start_filter();
    view.type_filter('m');
    assert_eq!(selected_name(&view), "gamma");
    assert_eq!(view.filter_status(40).as_deref(), Some("/m\u{2588} (1 of 3)"));
    view.clear_filter();
    assert_eq!(selected_name(&view), "gamma");
}

fn with_source(name: &str, source: ConnectionSource) -> ConnectionEntry {
    let mut entry = entry(name, None);
    entry.source = source;
    entry
}

fn selected_source(view: &ConnectionsView) -> Option<ConnectionSource> {
    view.selected_entry().map(|entry| entry.source)
}

#[test]
fn a_reload_keeps_the_cursor_on_the_profile_not_its_shadowed_labels() {
    let mut view = ConnectionsView::new();
    view.replace(vec![
        with_source("web1", ConnectionSource::Profile),
        with_source("web1", ConnectionSource::ShadowedSshHost),
    ]);
    assert_eq!(selected_source(&view), Some(ConnectionSource::Profile));

    view.replace(vec![
        with_source("web1", ConnectionSource::ShadowedSshHost),
        with_source("web1", ConnectionSource::Profile),
    ]);

    assert_eq!(selected_source(&view), Some(ConnectionSource::Profile));
}

#[test]
fn a_reload_keeps_the_cursor_on_shadowed_labels_not_the_profile() {
    let mut view = ConnectionsView::new();
    view.replace(vec![
        with_source("web1", ConnectionSource::Profile),
        with_source("web1", ConnectionSource::ShadowedSshHost),
    ]);
    view.move_cursor(1);

    view.replace(vec![
        with_source("aaa", ConnectionSource::Profile),
        with_source("web1", ConnectionSource::Profile),
        with_source("web1", ConnectionSource::ShadowedSshHost),
    ]);

    assert_eq!(selected_source(&view), Some(ConnectionSource::ShadowedSshHost));
}

#[test]
fn reveal_lands_on_the_profile_rather_than_same_named_labels() {
    let mut view = ConnectionsView::new();
    let mut shadowed = with_source("web1", ConnectionSource::ShadowedSshHost);
    shadowed.group = Some("Old".into());
    view.replace(vec![
        with_source("aaa", ConnectionSource::Profile),
        shadowed,
        with_source("web1", ConnectionSource::Profile),
    ]);

    view.reveal("web1");

    assert_eq!(selected_source(&view), Some(ConnectionSource::Profile));
}

#[test]
fn enter_on_a_group_while_filtering_leaves_its_state_alone() {
    let mut view = view();
    view.start_filter();
    view.type_filter('a');
    assert_eq!(selected_name(&view), "group:Work");

    view.toggle();
    view.expand();
    view.clear_filter();

    assert_eq!(view.rows().len(), 2);
}

#[test]
fn a_bare_hash_filter_lets_left_close_an_open_group() {
    let mut view = view();
    view.toggle();
    view.start_filter();
    view.type_filter('#');

    view.collapse_or_parent();

    assert_eq!(selected_name(&view), "group:Work");
    assert_eq!(view.rows().len(), 2);
}
