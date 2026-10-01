use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use porthmos_vfs::{ErrorKind, testing::FakeFs};

use super::*;
use crate::sync::TreeKind;

fn fs_with_tree() -> FakeFs {
    let fs = FakeFs::new();
    fs.file("/root/a.txt", b"12345", Some(100));
    fs.file("/root/sub/b.txt", b"123", Some(200));
    fs.file("/root/sub/deep/c.txt", b"1", None);
    fs.dir("/root/empty");
    fs
}

fn keys(scanned: &Scanned) -> Vec<(String, TreeKind)> {
    scanned.tree.iter().map(|(path, entry)| (path.display().to_string(), entry.kind)).collect()
}

#[tokio::test]
async fn a_recursive_scan_records_files_and_folders_with_their_facts() {
    let scanned = scan_tree(&fs_with_tree(), Path::new("/root"), true, &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(
        keys(&scanned),
        vec![
            ("a.txt".to_string(), TreeKind::File),
            ("empty".to_string(), TreeKind::Dir),
            ("sub".to_string(), TreeKind::Dir),
            ("sub/b.txt".to_string(), TreeKind::File),
            ("sub/deep".to_string(), TreeKind::Dir),
            ("sub/deep/c.txt".to_string(), TreeKind::File),
        ]
    );
    let a = scanned.tree[&PathBuf::from("a.txt")];
    assert_eq!((a.size, a.modified), (5, Some(100)));
    assert_eq!(scanned.tree[&PathBuf::from("sub/deep/c.txt")].modified, None);
}

#[tokio::test]
async fn without_subfolders_only_the_top_level_is_read() {
    let scanned =
        scan_tree(&fs_with_tree(), Path::new("/root"), false, &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(
        keys(&scanned),
        vec![
            ("a.txt".to_string(), TreeKind::File),
            ("empty".to_string(), TreeKind::Dir),
            ("sub".to_string(), TreeKind::Dir)
        ]
    );
}

#[tokio::test]
async fn symlinks_are_counted_and_recorded_as_symlinks_at_every_depth_and_whatever_they_point_to() {
    let fs = fs_with_tree();
    fs.symlink("/root/link", "/root/a.txt");
    fs.symlink("/root/sub/nested_link", "/root/sub/b.txt");
    fs.symlink("/root/folder_link", "/root/sub");

    let scanned = scan_tree(&fs, Path::new("/root"), true, &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(scanned.skipped_symlinks, 3);
    for linked in ["link", "sub/nested_link", "folder_link"] {
        let entry = scanned.tree[&PathBuf::from(linked)];
        assert_eq!((entry.kind, entry.size, entry.modified), (TreeKind::Symlink, 0, None), "{linked}");
    }
    assert_eq!(scanned.tree.values().filter(|entry| entry.kind != TreeKind::Symlink).count(), 6);
    assert_eq!(scanned.tree.len(), 9);
}

#[tokio::test]
async fn a_symlink_without_subfolders_is_recorded_too() {
    let fs = fs_with_tree();
    fs.symlink("/root/link", "/root/a.txt");

    let scanned = scan_tree(&fs, Path::new("/root"), false, &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(scanned.tree[&PathBuf::from("link")].kind, TreeKind::Symlink);
    assert_eq!(scanned.skipped_symlinks, 1);
}

#[test]
fn a_relative_path_of_plain_names_is_accepted() {
    assert!(check_relative(Path::new("a/b/c.txt")).is_ok());
    assert!(check_relative(Path::new("single")).is_ok());
}

#[test]
fn paths_that_could_leave_the_root_are_refused() {
    for hostile in ["..", "../x", "a/../../x", "/etc/passwd", "./a", ""] {
        let error = check_relative(Path::new(hostile)).err().unwrap_or_else(|| panic!("{hostile:?} was accepted"));
        assert_eq!(error.kind(), ErrorKind::Other, "{hostile:?}");
        assert!(error.to_string().contains("outside"), "{error}");
    }
}

#[tokio::test]
async fn a_cancelled_scan_returns_nothing() {
    let outcome = scan_tree(&fs_with_tree(), Path::new("/root"), true, &AtomicBool::new(true)).await.unwrap();

    assert!(outcome.is_none());
}

#[tokio::test]
async fn an_unreadable_subfolder_fails_the_whole_scan() {
    let fs = fs_with_tree();
    fs.fail_read_dir("/root/sub/deep");

    let error = scan_tree(&fs, Path::new("/root"), true, &AtomicBool::new(false)).await.err().unwrap();

    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn a_missing_root_is_an_error() {
    let error = scan_tree(&FakeFs::new(), Path::new("/nowhere"), true, &AtomicBool::new(false)).await.err().unwrap();

    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn a_missing_root_is_an_error_without_subfolders_too() {
    let error = scan_tree(&FakeFs::new(), Path::new("/nowhere"), false, &AtomicBool::new(false)).await.err().unwrap();

    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn an_empty_root_scans_to_an_empty_tree_even_when_cancelled() {
    let fs = FakeFs::new();
    fs.dir("/empty_root");

    let scanned = scan_tree(&fs, Path::new("/empty_root"), true, &AtomicBool::new(true)).await.unwrap().unwrap();

    assert!(scanned.tree.is_empty());
}

#[tokio::test]
async fn both_sides_are_scanned_together() {
    let local = fs_with_tree();
    let remote = FakeFs::new();
    remote.file("/srv/x.txt", b"1", Some(5));

    let (left, right) =
        scan_both(&local, Path::new("/root"), &remote, Path::new("/srv"), true, &AtomicBool::new(false))
            .await
            .unwrap()
            .unwrap();

    assert_eq!(left.tree.len(), 6);
    assert_eq!(keys(&right), vec![("x.txt".to_string(), TreeKind::File)]);
}

#[tokio::test]
async fn if_the_remote_side_fails_the_pair_fails() {
    let local = fs_with_tree();
    let remote = FakeFs::new();

    let error = scan_both(&local, Path::new("/root"), &remote, Path::new("/missing"), true, &AtomicBool::new(false))
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn if_the_local_side_fails_the_pair_fails() {
    let local = FakeFs::new();
    let remote = fs_with_tree();

    let error = scan_both(&local, Path::new("/missing"), &remote, Path::new("/root"), true, &AtomicBool::new(false))
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn if_either_side_is_cancelled_the_pair_is_cancelled() {
    let local = fs_with_tree();
    let remote = fs_with_tree();

    let result =
        scan_both(&local, Path::new("/root"), &remote, Path::new("/root"), true, &AtomicBool::new(true)).await.unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn if_only_the_local_side_is_cancelled_the_pair_is_cancelled() {
    let local = fs_with_tree();
    let remote = FakeFs::new();
    remote.dir("/empty_root");

    let result = scan_both(&local, Path::new("/root"), &remote, Path::new("/empty_root"), true, &AtomicBool::new(true))
        .await
        .unwrap();

    assert!(result.is_none());
}

#[tokio::test]
async fn if_only_the_remote_side_is_cancelled_the_pair_is_cancelled() {
    let local = FakeFs::new();
    local.dir("/empty_root");
    let remote = fs_with_tree();

    let result = scan_both(&local, Path::new("/empty_root"), &remote, Path::new("/root"), true, &AtomicBool::new(true))
        .await
        .unwrap();

    assert!(result.is_none());
}

fn found_with(directories: &[&str], files: &[&str], symlinks: &[&str]) -> crate::transfer::plan::DiscoveredTree {
    crate::transfer::plan::DiscoveredTree {
        directories: directories.iter().map(PathBuf::from).collect(),
        files: files.iter().map(|path| (PathBuf::from(path), 1, Some(1))).collect(),
        skipped_symlinks: symlinks.len(),
        symlinks: symlinks.iter().map(PathBuf::from).collect(),
    }
}

#[test]
fn building_a_tree_refuses_a_hostile_file_folder_or_symlink_name() {
    for found in [
        found_with(&[], &["ok.txt", "../escape.txt"], &[]),
        found_with(&["sub", "/abs"], &["ok.txt"], &[]),
        found_with(&[], &["ok.txt"], &["a/../../link"]),
    ] {
        let error = build_tree(found, true).err().expect("a hostile name must fail the scan");
        assert_eq!(error.kind(), ErrorKind::Other);
    }
}

#[test]
fn building_a_tree_from_ordinary_names_succeeds() {
    let scanned = build_tree(found_with(&["sub"], &["sub/a.txt"], &["sub/link"]), true).unwrap();

    assert_eq!(scanned.tree.len(), 3);
    assert_eq!(scanned.skipped_symlinks, 1);
}

fn refusal_of(found: crate::transfer::plan::DiscoveredTree) -> ProtocolError {
    build_tree(found, true).err().expect("a path without its parent folder must fail the scan")
}

#[test]
fn a_file_whose_parent_folder_is_not_in_the_tree_is_refused_and_named() {
    let error = refusal_of(found_with(&[], &["a/b"], &[]));

    assert_eq!(error.kind(), ErrorKind::Other);
    assert!(error.to_string().contains("a/b"), "{error}");
}

#[test]
fn a_folder_whose_parent_folder_is_not_in_the_tree_is_refused() {
    assert_eq!(refusal_of(found_with(&["a/b"], &[], &[])).kind(), ErrorKind::Other);
}

#[test]
fn a_symlink_whose_parent_folder_is_not_in_the_tree_is_refused() {
    assert_eq!(refusal_of(found_with(&[], &[], &["a/link"])).kind(), ErrorKind::Other);
}

#[test]
fn a_missing_folder_in_the_middle_of_a_deep_path_is_refused() {
    assert_eq!(refusal_of(found_with(&["a", "a/b/c"], &[], &[])).kind(), ErrorKind::Other);
}

#[test]
fn a_parent_that_is_a_symlink_or_a_file_is_not_a_folder() {
    assert_eq!(refusal_of(found_with(&[], &["a/b"], &["a"])).kind(), ErrorKind::Other);
    assert_eq!(refusal_of(found_with(&[], &["a", "a/b"], &[])).kind(), ErrorKind::Other);
}

#[test]
fn paths_whose_parents_are_folders_in_the_tree_are_accepted() {
    let scanned =
        build_tree(found_with(&["a", "a/b"], &["a/b/c.txt", "top.txt"], &["a/link", "toplink"]), true).unwrap();

    assert_eq!(scanned.tree.len(), 6);
}

#[test]
fn without_subfolders_a_nested_path_of_any_kind_fails_the_scan() {
    for found in [
        found_with(&["a", "a/s"], &["a/s/x"], &[]),
        found_with(&["a", "a/s"], &[], &[]),
        found_with(&["a"], &["a/x"], &[]),
        found_with(&["a"], &[], &["a/link"]),
    ] {
        let error = build_tree(found, false).err().expect("a nested path must fail a scan without subfolders");
        assert_eq!(error.kind(), ErrorKind::Other);
        assert!(error.to_string().contains("a/"), "{error}");
    }
}

#[test]
fn without_subfolders_depth_one_paths_of_every_kind_are_accepted() {
    let scanned = build_tree(found_with(&["a"], &["top.txt"], &["link"]), false).unwrap();

    assert_eq!(scanned.tree.len(), 3);
}

#[test]
fn with_subfolders_the_same_nested_paths_are_accepted_when_the_parents_exist() {
    let scanned = build_tree(found_with(&["a", "a/s"], &["a/s/x"], &[]), true).unwrap();

    assert_eq!(scanned.tree.len(), 3);
}

#[test]
fn two_listed_names_that_make_the_same_path_fail_the_scan() {
    for found in [
        found_with(&["a"], &["a/"], &[]),
        found_with(&[], &["a", "a/"], &[]),
        found_with(&["a"], &[], &["a/."]),
        found_with(&[], &["a"], &["a/"]),
        found_with(&["a", "a/"], &[], &[]),
    ] {
        let error = build_tree(found, true).err().expect("a duplicated path must fail the scan");
        assert_eq!(error.kind(), ErrorKind::Other);
    }
}

#[tokio::test]
async fn a_real_nested_folder_is_not_entered_when_subfolders_is_off() {
    let fs = FakeFs::new();
    fs.file("/root/top.txt", b"1", Some(1));
    fs.file("/root/a/s/x.txt", b"2", Some(1));

    let scanned = scan_tree(&fs, Path::new("/root"), false, &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(keys(&scanned), vec![("a".to_string(), TreeKind::Dir), ("top.txt".to_string(), TreeKind::File)]);
}
