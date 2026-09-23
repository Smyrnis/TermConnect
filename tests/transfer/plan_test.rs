use std::os::unix::fs::symlink;

use super::*;

#[test]
fn discover_local_tree_finds_files_at_every_depth() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("top.txt"), b"a").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/nested.txt"), b"bb").unwrap();

    let tree = discover_local_tree(dir.path(), &AtomicBool::new(false)).unwrap().unwrap();

    let mut files: Vec<(PathBuf, u64)> = tree.files.into_iter().map(|(path, size, _)| (path, size)).collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(files, vec![(PathBuf::from("sub/nested.txt"), 2), (PathBuf::from("top.txt"), 1),]);
}

#[test]
fn discover_local_tree_lists_directories_parent_before_child() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a/b")).unwrap();

    let tree = discover_local_tree(dir.path(), &AtomicBool::new(false)).unwrap().unwrap();

    assert_eq!(tree.directories, vec![PathBuf::from("a"), PathBuf::from("a/b")]);
}

#[test]
fn discover_local_tree_includes_empty_directories() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("empty")).unwrap();

    let tree = discover_local_tree(dir.path(), &AtomicBool::new(false)).unwrap().unwrap();

    assert_eq!(tree.directories, vec![PathBuf::from("empty")]);
    assert!(tree.files.is_empty());
}

#[test]
fn discover_local_tree_skips_symlinks_and_counts_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("real.txt"), b"a").unwrap();
    symlink(dir.path().join("real.txt"), dir.path().join("link.txt")).unwrap();

    let tree = discover_local_tree(dir.path(), &AtomicBool::new(false)).unwrap().unwrap();

    assert_eq!(tree.files.into_iter().map(|(path, size, _)| (path, size)).collect::<Vec<_>>(), vec![(PathBuf::from("real.txt"), 1)]);
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
    let tree = DiscoveredTree { directories: vec![PathBuf::from("sub")], files: vec![(PathBuf::from("top.txt"), 5, Some(50)), (PathBuf::from("sub/nested.txt"), 7, None)], skipped_symlinks: 0 };

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
    let tree = DiscoveredTree { directories: vec![PathBuf::from("sub")], files: vec![(PathBuf::from("top.txt"), 5, Some(50)), (PathBuf::from("sub/nested.txt"), 7, None)], skipped_symlinks: 0 };

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

#[test]
fn discover_local_tree_returns_none_when_already_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/nested.txt"), b"a").unwrap();

    let tree = discover_local_tree(dir.path(), &AtomicBool::new(true)).unwrap();

    assert!(tree.is_none());
}

fn planned(local: &str, remote: &str) -> PlannedFile {
    PlannedFile { local_path: PathBuf::from(local), remote_path: remote.to_string(), display_name: "x".to_string(), size: 1, existing: None, source_modified: None }
}

fn existing(is_dir: bool) -> ExistingFile {
    ExistingFile { size: 7, modified: Some(100), is_dir }
}

#[test]
fn mark_conflicts_flags_files_whose_name_exists_at_the_destination() {
    let mut files = vec![planned("/local/a.txt", "/remote/dest/a.txt"), planned("/local/b.txt", "/remote/dest/b.txt"), planned("/local/c", "/remote/dest/c")];
    let listing: DestinationListing = [("a.txt".to_string(), existing(false)), ("c".to_string(), existing(true))].into_iter().collect();
    let listings: HashMap<PathBuf, DestinationListing> = [(PathBuf::from("/remote/dest"), listing)].into_iter().collect();

    mark_conflicts(Direction::Upload, &mut files, &listings);

    assert_eq!(files[0].existing, Some(existing(false)));
    assert_eq!(files[1].existing, None);
    assert_eq!(files[2].existing, Some(existing(true)));
}

#[test]
fn mark_conflicts_uses_the_local_side_for_downloads_and_treats_unlisted_folders_as_empty() {
    let mut files = vec![planned("/local/dest/a.txt", "/remote/a.txt"), planned("/local/other/a.txt", "/remote/b.txt")];
    let listing: DestinationListing = [("a.txt".to_string(), existing(false))].into_iter().collect();
    let listings: HashMap<PathBuf, DestinationListing> = [(PathBuf::from("/local/dest"), listing)].into_iter().collect();

    mark_conflicts(Direction::Download, &mut files, &listings);

    assert!(files[0].existing.is_some());
    assert!(files[1].existing.is_none());
}

#[test]
fn parents_created_by_the_scan_are_not_listed() {
    let files = vec![planned("/l/a", "/remote/dest/a"), planned("/l/b", "/remote/dest/new/b"), planned("/l/c", "/remote/dest/new/sub/c"), planned("/l/d", "/remote/dest/d")];
    let created: HashSet<PathBuf> = [PathBuf::from("/remote/dest/new"), PathBuf::from("/remote/dest/new/sub")].into_iter().collect();

    assert_eq!(parents_needing_listing(Direction::Upload, &files, &created), vec![PathBuf::from("/remote/dest")]);
}

#[test]
fn taken_names_combine_existing_and_planned_names_per_folder() {
    let files = vec![planned("/l/a", "/remote/dest/a.txt"), planned("/l/b", "/remote/dest/b.txt")];
    let listing: DestinationListing = [("old.txt".to_string(), existing(false))].into_iter().collect();
    let listings: HashMap<PathBuf, DestinationListing> = [(PathBuf::from("/remote/dest"), listing)].into_iter().collect();

    let taken = taken_names(Direction::Upload, &files, &listings);

    let names = &taken[&PathBuf::from("/remote/dest")];
    assert_eq!(names.len(), 3);
    assert!(names.contains("old.txt") && names.contains("a.txt") && names.contains("b.txt"));
}

#[test]
fn list_local_destination_reports_files_and_folders_and_treats_missing_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();

    let listing = list_local_destination(dir.path()).unwrap();
    let missing = list_local_destination(&dir.path().join("nope")).unwrap();

    assert_eq!(listing["a.txt"].size, 5);
    assert!(!listing["a.txt"].is_dir);
    assert!(listing["a.txt"].modified.is_some());
    assert!(listing["sub"].is_dir);
    assert!(missing.is_empty());
}

#[test]
fn ensure_local_directory_reports_whether_it_created_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new");

    assert!(ensure_local_directory(&target).unwrap());
    assert!(!ensure_local_directory(&target).unwrap());
}

#[test]
fn discover_local_tree_records_modification_times() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"a").unwrap();

    let tree = discover_local_tree(dir.path(), &AtomicBool::new(false)).unwrap().unwrap();

    assert!(tree.files[0].2.is_some());
}

#[test]
fn planned_files_for_tree_carries_the_source_modification_time() {
    let entry = sample_entry("myfolder", "/local/myfolder", true, 0);
    let tree = DiscoveredTree { directories: Vec::new(), files: vec![(PathBuf::from("top.txt"), 5, Some(50))], skipped_symlinks: 0 };

    let planned = planned_files_for_tree(Direction::Upload, &entry, &PathBuf::from("/remote/dest/myfolder"), &tree);

    assert_eq!(planned[0].source_modified, Some(50));
}

#[test]
fn list_local_destination_sees_through_symlinks_and_keeps_dangling_ones() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("real_folder")).unwrap();
    symlink(dir.path().join("real_folder"), dir.path().join("photos")).unwrap();
    symlink(dir.path().join("missing"), dir.path().join("dangling")).unwrap();

    let listing = list_local_destination(dir.path()).unwrap();

    assert!(listing["photos"].is_dir);
    assert!(!listing["dangling"].is_dir);
}
