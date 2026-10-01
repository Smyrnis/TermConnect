use std::path::PathBuf;

use super::*;

fn file(size: u64, modified: Option<u64>) -> TreeEntry {
    TreeEntry { kind: TreeKind::File, size, modified }
}

fn dir() -> TreeEntry {
    TreeEntry { kind: TreeKind::Dir, size: 0, modified: None }
}

fn tree(entries: &[(&str, TreeEntry)]) -> Tree {
    entries.iter().map(|(path, entry)| (PathBuf::from(path), *entry)).collect()
}

fn options(direction: SyncDirection, by: SyncBy) -> SyncOptions {
    SyncOptions { direction, by, subfolders: true }
}

type Row = (String, SyncAction, SyncReason, bool);

fn diff(local: &Tree, remote: &Tree, options: SyncOptions, tolerance: u64) -> Vec<SyncItem> {
    compare(local, remote, options, tolerance, true)
}

fn rows(items: &[SyncItem]) -> Vec<Row> {
    items.iter().map(|item| (item.path.display().to_string(), item.action, item.reason, item.ticked)).collect()
}

fn run(local: &[(&str, TreeEntry)], remote: &[(&str, TreeEntry)], options: SyncOptions) -> Vec<Row> {
    rows(&diff(&tree(local), &tree(remote), options, 2))
}

use SyncAction::{Download, Skip, Upload};
use SyncBy::{Size, Time};
use SyncDirection::{Both, LocalToRemote, RemoteToLocal};
use SyncReason::*;

#[test]
fn a_file_only_on_the_source_is_copied_and_ticked() {
    let up = run(&[("a", file(5, Some(100)))], &[], options(LocalToRemote, Time));
    let down = run(&[], &[("a", file(5, Some(100)))], options(RemoteToLocal, Time));

    assert_eq!(up, vec![("a".into(), Upload, OnlyLocal, true)]);
    assert_eq!(down, vec![("a".into(), Download, OnlyRemote, true)]);
}

#[test]
fn a_file_only_on_the_target_is_never_listed_in_one_way() {
    let up = run(&[], &[("a", file(5, Some(100)))], options(LocalToRemote, Time));
    let down = run(&[("a", file(5, Some(100)))], &[], options(RemoteToLocal, Time));

    assert!(up.is_empty() && down.is_empty());
}

#[test]
fn identical_files_are_not_listed() {
    let same = run(&[("a", file(5, Some(100)))], &[("a", file(5, Some(100)))], options(LocalToRemote, Time));
    let within_tolerance = run(&[("a", file(5, Some(100)))], &[("a", file(5, Some(102)))], options(Both, Time));

    assert!(same.is_empty() && within_tolerance.is_empty());
}

#[test]
fn a_newer_source_is_copied_and_ticked() {
    let up = run(&[("a", file(5, Some(200)))], &[("a", file(5, Some(100)))], options(LocalToRemote, Time));
    let down = run(&[("a", file(5, Some(100)))], &[("a", file(5, Some(200)))], options(RemoteToLocal, Time));

    assert_eq!(up, vec![("a".into(), Upload, LocalNewer, true)]);
    assert_eq!(down, vec![("a".into(), Download, RemoteNewer, true)]);
}

#[test]
fn a_newer_target_is_listed_but_skipped_and_unticked() {
    let up = run(&[("a", file(5, Some(100)))], &[("a", file(5, Some(200)))], options(LocalToRemote, Time));
    let down = run(&[("a", file(5, Some(200)))], &[("a", file(5, Some(100)))], options(RemoteToLocal, Time));

    assert_eq!(up, vec![("a".into(), Skip, TargetNewer, false)]);
    assert_eq!(down, vec![("a".into(), Skip, TargetNewer, false)]);
}

#[test]
fn the_tolerance_boundary_is_inclusive() {
    let at_tolerance = run(&[("a", file(5, Some(102)))], &[("a", file(5, Some(100)))], options(LocalToRemote, Time));
    let past_tolerance = run(&[("a", file(5, Some(103)))], &[("a", file(5, Some(100)))], options(LocalToRemote, Time));

    assert!(at_tolerance.is_empty());
    assert_eq!(past_tolerance, vec![("a".into(), Upload, LocalNewer, true)]);
}

#[test]
fn a_larger_tolerance_hides_a_larger_difference() {
    let items = diff(
        &tree(&[("a", file(5, Some(150)))]),
        &tree(&[("a", file(5, Some(100)))]),
        options(LocalToRemote, Time),
        60,
    );

    assert!(items.is_empty());
}

#[test]
fn the_same_time_with_a_different_size_is_suspicious_and_unticked() {
    let one_way = run(&[("a", file(5, Some(100)))], &[("a", file(9, Some(100)))], options(LocalToRemote, Time));
    let both = run(&[("a", file(5, Some(100)))], &[("a", file(9, Some(101)))], options(Both, Time));

    assert_eq!(one_way, vec![("a".into(), Skip, SameTimeDifferentSize, false)]);
    assert_eq!(both, vec![("a".into(), Skip, SameTimeDifferentSize, false)]);
}

#[test]
fn comparing_by_size_copies_files_whose_size_differs_whatever_their_times() {
    let differs = run(&[("a", file(5, Some(100)))], &[("a", file(9, Some(900)))], options(LocalToRemote, Size));
    let equal = run(&[("a", file(5, Some(100)))], &[("a", file(5, Some(900)))], options(LocalToRemote, Size));
    let no_time = run(&[("a", file(5, None))], &[("a", file(9, None))], options(RemoteToLocal, Size));

    assert_eq!(differs, vec![("a".into(), Upload, SizeDiffers, true)]);
    assert!(equal.is_empty());
    assert_eq!(no_time, vec![("a".into(), Download, SizeDiffers, true)]);
}

#[test]
fn an_unknown_time_falls_back_to_the_size() {
    let equal = run(&[("a", file(5, None))], &[("a", file(5, Some(100)))], options(LocalToRemote, Time));
    let differs = run(&[("a", file(5, None))], &[("a", file(9, Some(100)))], options(LocalToRemote, Time));
    let both_unknown = run(&[("a", file(5, None))], &[("a", file(9, None))], options(RemoteToLocal, Time));

    assert!(equal.is_empty());
    assert_eq!(differs, vec![("a".into(), Upload, SizeDiffersTimeUnknown, true)]);
    assert_eq!(both_unknown, vec![("a".into(), Download, SizeDiffersTimeUnknown, true)]);
}

#[test]
fn both_copies_missing_and_newer_files_in_the_right_direction() {
    let items = run(
        &[("only_local", file(1, Some(10))), ("newer_local", file(1, Some(500))), ("older_local", file(1, Some(100)))],
        &[("only_remote", file(1, Some(10))), ("newer_local", file(1, Some(100))), ("older_local", file(1, Some(500)))],
        options(Both, Time),
    );

    assert_eq!(
        items,
        vec![
            ("newer_local".into(), Upload, LocalNewer, true),
            ("older_local".into(), Download, RemoteNewer, true),
            ("only_local".into(), Upload, OnlyLocal, true),
            ("only_remote".into(), Download, OnlyRemote, true),
        ]
    );
}

#[test]
fn both_with_an_unknown_time_and_a_different_size_asks_the_user() {
    let items = run(&[("a", file(5, None))], &[("a", file(9, Some(100)))], options(Both, Time));

    assert_eq!(items, vec![("a".into(), Skip, SizeDiffersTimeUnknown, false)]);
}

#[test]
fn a_file_against_a_folder_is_a_mismatch_that_hides_everything_below_it() {
    let items = run(
        &[("x", file(5, Some(1))), ("y", dir()), ("y/inner", file(1, Some(1)))],
        &[("x", dir()), ("x/child", file(1, Some(1))), ("x/child2", file(2, Some(2))), ("y", file(3, Some(1)))],
        options(Both, Time),
    );

    assert_eq!(items, vec![("x".into(), Skip, KindMismatch, false), ("y".into(), Skip, KindMismatch, false)]);
}

#[test]
fn two_folders_with_the_same_name_produce_no_item() {
    let items = run(&[("d", dir())], &[("d", dir())], options(LocalToRemote, Time));

    assert!(items.is_empty());
}

#[test]
fn a_folder_that_exists_on_one_side_only_produces_only_its_files() {
    let items = run(&[("d", dir()), ("d/a", file(1, Some(1))), ("d/e", dir())], &[], options(LocalToRemote, Time));

    assert_eq!(items, vec![("d/a".into(), Upload, OnlyLocal, true)]);
}

#[test]
fn items_are_sorted_by_path_and_numbered_from_zero() {
    let plan = diff(
        &tree(&[("b", file(1, Some(1))), ("a", file(1, Some(1))), ("c/d", file(1, Some(1)))]),
        &Tree::new(),
        options(LocalToRemote, Time),
        2,
    );

    let order: Vec<(u32, String)> = plan.iter().map(|item| (item.id, item.path.display().to_string())).collect();
    assert_eq!(order, vec![(0, "a".into()), (1, "b".into()), (2, "c/d".into())]);
}

#[test]
fn the_facts_of_both_sides_are_kept_on_the_item() {
    let items =
        diff(&tree(&[("a", file(5, Some(200)))]), &tree(&[("a", file(9, Some(100)))]), options(LocalToRemote, Time), 2);

    assert_eq!(items[0].local, Some(SyncFacts { size: 5, modified: Some(200) }));
    assert_eq!(items[0].remote, Some(SyncFacts { size: 9, modified: Some(100) }));
}

#[test]
fn a_target_that_cannot_keep_times_does_not_list_same_size_files_it_merely_stamped_newer() {
    let source = tree(&[("a", file(5, Some(100)))]);
    let target = tree(&[("a", file(5, Some(9_000)))]);

    let keeps = compare(&source, &target, options(LocalToRemote, Time), 2, true);
    let does_not_keep = compare(&source, &target, options(LocalToRemote, Time), 2, false);

    assert_eq!(rows(&keeps), vec![("a".into(), Skip, TargetNewer, false)]);
    assert!(does_not_keep.is_empty());
}

#[test]
fn a_target_that_cannot_keep_times_still_lists_a_newer_file_of_a_different_size() {
    let source = tree(&[("a", file(5, Some(100)))]);
    let target = tree(&[("a", file(9, Some(9_000)))]);

    let items = compare(&source, &target, options(LocalToRemote, Time), 2, false);

    assert_eq!(rows(&items), vec![("a".into(), Skip, TargetNewer, false)]);
}

#[test]
fn a_source_changed_after_the_target_was_stamped_is_still_found() {
    let source = tree(&[("a", file(5, Some(10_000)))]);
    let target = tree(&[("a", file(5, Some(9_000)))]);

    let items = compare(&source, &target, options(LocalToRemote, Time), 2, false);

    assert_eq!(rows(&items), vec![("a".into(), Upload, LocalNewer, true)]);
}

#[test]
fn both_cannot_compare_by_size() {
    assert!(options(Both, Size).validate().is_err());
    assert!(options(Both, Time).validate().is_ok());
    assert!(options(LocalToRemote, Size).validate().is_ok());
    assert!(options(RemoteToLocal, Size).validate().is_ok());
}

#[test]
fn only_the_ambiguous_rows_are_flippable() {
    let items = diff(
        &tree(&[
            ("new", file(1, Some(1))),
            ("newer", file(1, Some(500))),
            ("target_newer", file(1, Some(1))),
            ("odd", file(1, Some(100))),
            ("kind", file(1, Some(1))),
        ]),
        &tree(&[
            ("newer", file(1, Some(100))),
            ("target_newer", file(1, Some(500))),
            ("odd", file(2, Some(100))),
            ("kind", dir()),
        ]),
        options(LocalToRemote, Time),
        2,
    );

    let flippable: Vec<(String, bool)> =
        items.iter().map(|item| (item.path.display().to_string(), item.flippable)).collect();
    assert_eq!(
        flippable,
        vec![
            ("kind".into(), false),
            ("new".into(), false),
            ("newer".into(), false),
            ("odd".into(), true),
            ("target_newer".into(), true),
        ]
    );
}

fn item_for(local: Option<SyncFacts>, remote: Option<SyncFacts>, action: SyncAction, reason: SyncReason) -> SyncItem {
    let flippable = matches!(reason, TargetNewer | SameTimeDifferentSize | SizeDiffersTimeUnknown);
    SyncItem { id: 0, path: PathBuf::from("a"), local, remote, action, reason, ticked: false, flippable }
}

fn facts() -> Option<SyncFacts> {
    Some(SyncFacts { size: 1, modified: Some(1) })
}

#[test]
fn one_way_only_ever_allows_its_own_direction() {
    let item = item_for(facts(), facts(), Skip, TargetNewer);

    assert_eq!(item.allowed_actions(LocalToRemote), vec![Skip, Upload]);
    assert_eq!(item.allowed_actions(RemoteToLocal), vec![Skip, Download]);
}

#[test]
fn both_allows_the_directions_the_files_make_possible() {
    let both_sides = item_for(facts(), facts(), Skip, SameTimeDifferentSize);
    let only_local = item_for(facts(), None, Upload, OnlyLocal);
    let only_remote = item_for(None, facts(), Download, OnlyRemote);

    assert_eq!(both_sides.allowed_actions(Both), vec![Skip, Upload, Download]);
    assert_eq!(only_local.allowed_actions(Both), vec![Skip, Upload]);
    assert_eq!(only_remote.allowed_actions(Both), vec![Skip, Download]);
}

#[test]
fn a_file_against_a_folder_allows_nothing_but_skip() {
    let item = item_for(facts(), None, Skip, KindMismatch);

    assert_eq!(item.allowed_actions(Both), vec![Skip]);
    assert_eq!(item.allowed_actions(LocalToRemote), vec![Skip]);
    assert!(!item.allows(Upload, LocalToRemote));
}

#[test]
fn next_action_cycles_through_the_allowed_ones_and_stays_put_when_there_is_one() {
    let item = item_for(facts(), facts(), Skip, SameTimeDifferentSize);
    let fixed = item_for(facts(), None, Skip, KindMismatch);

    assert_eq!(item.next_action(Both), Upload);
    let mut flipped = item.clone();
    flipped.action = Upload;
    assert_eq!(flipped.next_action(Both), Download);
    flipped.action = Download;
    assert_eq!(flipped.next_action(Both), Skip);
    assert_eq!(fixed.next_action(Both), Skip);
}

#[test]
fn action_after_cycles_from_any_starting_action() {
    let item = item_for(facts(), facts(), Skip, SameTimeDifferentSize);

    assert_eq!(item.action_after(Skip, Both), Upload);
    assert_eq!(item.action_after(Upload, Both), Download);
    assert_eq!(item.action_after(Download, Both), Skip);
    assert_eq!(item.action_after(Download, LocalToRemote), Skip);
}

#[test]
fn a_hundred_thousand_files_compare_quickly() {
    let mut local = Tree::new();
    let mut remote = Tree::new();
    for index in 0..100_000u64 {
        let path = format!("dir{}/sub{}/file{index}", index % 50, index % 7);
        local.insert(PathBuf::from(&path), file(index, Some(1_000)));
        let remote_time = if index % 10 == 0 { 500 } else { 1_000 };
        remote.insert(PathBuf::from(&path), file(index, Some(remote_time)));
    }
    let started = std::time::Instant::now();

    let items = diff(&local, &remote, options(LocalToRemote, Time), 2);

    assert_eq!(items.len(), 10_000);
    assert!(items.iter().all(|item| item.action == Upload && item.ticked));
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
}

#[test]
fn in_both_the_suspicious_rows_are_unticked_skips_the_user_can_flip() {
    let items = diff(
        &tree(&[("odd", file(1, Some(100))), ("unknown", file(1, None))]),
        &tree(&[("odd", file(2, Some(101))), ("unknown", file(2, Some(100)))]),
        options(Both, Time),
        2,
    );

    assert_eq!(
        rows(&items),
        vec![
            ("odd".into(), Skip, SameTimeDifferentSize, false),
            ("unknown".into(), Skip, SizeDiffersTimeUnknown, false),
        ]
    );
    assert!(items.iter().all(|item| item.flippable));
}

#[test]
fn a_one_way_copy_chosen_because_the_time_is_unknown_is_ticked_and_not_flippable() {
    let items =
        diff(&tree(&[("a", file(1, None))]), &tree(&[("a", file(2, Some(1)))]), options(LocalToRemote, Time), 2);

    assert_eq!(rows(&items), vec![("a".into(), Upload, SizeDiffersTimeUnknown, true)]);
    assert!(!items[0].flippable);
}

#[test]
fn everything_at_any_depth_below_a_mismatch_is_hidden() {
    let items = run(
        &[
            ("x", dir()),
            ("x/sub", dir()),
            ("x/sub/deep", file(1, Some(1))),
            ("x/sub/deeper", dir()),
            ("x/sub/deeper/leaf", file(1, Some(1))),
        ],
        &[("x", file(5, Some(1)))],
        options(Both, Time),
    );

    assert_eq!(items, vec![("x".into(), Skip, KindMismatch, false)]);
}

#[test]
fn a_remote_to_local_target_that_cannot_keep_times_ignores_same_size_files_it_merely_stamped_newer() {
    let source = tree(&[("a", file(5, Some(100)))]);
    let same_size_target = tree(&[("a", file(5, Some(9_000)))]);
    let other_size_target = tree(&[("a", file(9, Some(9_000)))]);

    let keeps = compare(&same_size_target, &source, options(RemoteToLocal, Time), 2, true);
    let does_not_keep = compare(&same_size_target, &source, options(RemoteToLocal, Time), 2, false);
    let other_size = compare(&other_size_target, &source, options(RemoteToLocal, Time), 2, false);

    assert_eq!(rows(&keeps), vec![("a".into(), Skip, TargetNewer, false)]);
    assert!(does_not_keep.is_empty());
    assert_eq!(rows(&other_size), vec![("a".into(), Skip, TargetNewer, false)]);
}

#[test]
fn comparing_by_size_in_both_directions_lists_nothing() {
    let items = diff(
        &tree(&[("a", file(1, Some(1))), ("b", file(1, Some(1)))]),
        &tree(&[("a", file(2, Some(1))), ("c", file(1, Some(1)))]),
        options(Both, Size),
        2,
    );

    assert!(items.is_empty());
}
