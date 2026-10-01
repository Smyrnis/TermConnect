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
async fn symlinks_are_skipped_and_counted_at_every_depth_and_whatever_they_point_to() {
    let fs = fs_with_tree();
    fs.symlink("/root/link", "/root/a.txt");
    fs.symlink("/root/sub/nested_link", "/root/sub/b.txt");
    fs.symlink("/root/folder_link", "/root/sub");

    let scanned = scan_tree(&fs, Path::new("/root"), true, &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(scanned.skipped_symlinks, 3);
    for skipped in ["link", "sub/nested_link", "folder_link"] {
        assert!(!scanned.tree.contains_key(&PathBuf::from(skipped)), "{skipped} must not be in the tree");
    }
    assert_eq!(scanned.tree.len(), 6);
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
