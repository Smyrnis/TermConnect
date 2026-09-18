use std::os::unix::fs::symlink;

use super::*;

#[test]
fn discover_local_tree_finds_files_at_every_depth() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("top.txt"), b"a").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/nested.txt"), b"bb").unwrap();

    let tree = discover_local_tree(dir.path()).unwrap();

    let mut files: Vec<(PathBuf, u64)> = tree.files;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        files,
        vec![
            (PathBuf::from("sub/nested.txt"), 2),
            (PathBuf::from("top.txt"), 1),
        ]
    );
}

#[test]
fn discover_local_tree_lists_directories_parent_before_child() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a/b")).unwrap();

    let tree = discover_local_tree(dir.path()).unwrap();

    assert_eq!(
        tree.directories,
        vec![PathBuf::from("a"), PathBuf::from("a/b")]
    );
}

#[test]
fn discover_local_tree_includes_empty_directories() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("empty")).unwrap();

    let tree = discover_local_tree(dir.path()).unwrap();

    assert_eq!(tree.directories, vec![PathBuf::from("empty")]);
    assert!(tree.files.is_empty());
}

#[test]
fn discover_local_tree_skips_symlinks_and_counts_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("real.txt"), b"a").unwrap();
    symlink(dir.path().join("real.txt"), dir.path().join("link.txt")).unwrap();

    let tree = discover_local_tree(dir.path()).unwrap();

    assert_eq!(tree.files, vec![(PathBuf::from("real.txt"), 1)]);
    assert_eq!(tree.skipped_symlinks, 1);
}

#[test]
fn ensure_local_directory_creates_a_missing_directory() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new");

    ensure_local_directory(&target).unwrap();

    assert!(target.is_dir());
}

#[test]
fn ensure_local_directory_is_a_no_op_when_it_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("existing");
    std::fs::create_dir(&target).unwrap();

    ensure_local_directory(&target).unwrap();

    assert!(target.is_dir());
}
