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
    assert_eq!(files, vec![(PathBuf::from("sub/nested.txt"), 2), (PathBuf::from("top.txt"), 1),]);
}

#[test]
fn discover_local_tree_lists_directories_parent_before_child() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a/b")).unwrap();

    let tree = discover_local_tree(dir.path()).unwrap();

    assert_eq!(tree.directories, vec![PathBuf::from("a"), PathBuf::from("a/b")]);
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

fn sample_entry(name: &str, path: &str, is_dir: bool, size: u64) -> Entry {
    Entry { name: name.to_string(), path: PathBuf::from(path), is_dir, size, permissions: None }
}

#[test]
fn planned_file_for_loose_entry_maps_paths_for_an_upload() {
    let entry = sample_entry("a.txt", "/local/a.txt", false, 10);
    let dest_dir = PathBuf::from("/remote/dest");

    let planned = planned_file_for_loose_entry(Direction::Upload, &entry, &dest_dir);

    assert_eq!(planned.local_path, PathBuf::from("/local/a.txt"));
    assert_eq!(planned.remote_path, "/remote/dest/a.txt");
    assert_eq!(planned.display_name, "a.txt");
    assert_eq!(planned.size, 10);
}

#[test]
fn planned_file_for_loose_entry_maps_paths_for_a_download() {
    let entry = sample_entry("a.txt", "/remote/a.txt", false, 10);
    let dest_dir = PathBuf::from("/local/dest");

    let planned = planned_file_for_loose_entry(Direction::Download, &entry, &dest_dir);

    assert_eq!(planned.local_path, PathBuf::from("/local/dest/a.txt"));
    assert_eq!(planned.remote_path, "/remote/a.txt");
    assert_eq!(planned.display_name, "a.txt");
    assert_eq!(planned.size, 10);
}

#[test]
fn planned_files_for_tree_maps_paths_for_an_upload_including_a_nested_file() {
    let entry = sample_entry("myfolder", "/local/myfolder", true, 0);
    let dest_root = PathBuf::from("/remote/dest/myfolder");
    let tree = DiscoveredTree {
        directories: vec![PathBuf::from("sub")],
        files: vec![(PathBuf::from("top.txt"), 5), (PathBuf::from("sub/nested.txt"), 7)],
        skipped_symlinks: 0,
    };

    let mut planned = planned_files_for_tree(Direction::Upload, &entry, &dest_root, &tree);
    planned.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    assert_eq!(planned.len(), 2);

    assert_eq!(planned[0].local_path, PathBuf::from("/local/myfolder/sub/nested.txt"));
    assert_eq!(planned[0].remote_path, "/remote/dest/myfolder/sub/nested.txt");
    assert_eq!(planned[0].display_name, "sub/nested.txt");
    assert_eq!(planned[0].size, 7);

    assert_eq!(planned[1].local_path, PathBuf::from("/local/myfolder/top.txt"));
    assert_eq!(planned[1].remote_path, "/remote/dest/myfolder/top.txt");
    assert_eq!(planned[1].display_name, "top.txt");
    assert_eq!(planned[1].size, 5);
}

#[test]
fn planned_files_for_tree_maps_paths_for_a_download_including_a_nested_file() {
    let entry = sample_entry("myfolder", "/remote/myfolder", true, 0);
    let dest_root = PathBuf::from("/local/dest/myfolder");
    let tree = DiscoveredTree {
        directories: vec![PathBuf::from("sub")],
        files: vec![(PathBuf::from("top.txt"), 5), (PathBuf::from("sub/nested.txt"), 7)],
        skipped_symlinks: 0,
    };

    let mut planned = planned_files_for_tree(Direction::Download, &entry, &dest_root, &tree);
    planned.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    assert_eq!(planned.len(), 2);

    assert_eq!(planned[0].local_path, PathBuf::from("/local/dest/myfolder/sub/nested.txt"));
    assert_eq!(planned[0].remote_path, "/remote/myfolder/sub/nested.txt");
    assert_eq!(planned[0].display_name, "sub/nested.txt");
    assert_eq!(planned[0].size, 7);

    assert_eq!(planned[1].local_path, PathBuf::from("/local/dest/myfolder/top.txt"));
    assert_eq!(planned[1].remote_path, "/remote/myfolder/top.txt");
    assert_eq!(planned[1].display_name, "top.txt");
    assert_eq!(planned[1].size, 5);
}
