use std::os::unix::fs::symlink;

use porthmos_lfs::LocalFs;
use porthmos_vfs::testing::FakeFs;

use super::*;

fn local() -> LocalFs {
    LocalFs::new(PathBuf::from("/"))
}

#[tokio::test]
async fn discover_local_tree_finds_files_at_every_depth() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("top.txt"), b"a").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/nested.txt"), b"bb").unwrap();

    let tree = discover_tree(&local(), dir.path(), &AtomicBool::new(false)).await.unwrap().unwrap();

    let mut files: Vec<(PathBuf, u64)> = tree.files.into_iter().map(|(path, size, _)| (path, size)).collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(files, vec![(PathBuf::from("sub/nested.txt"), 2), (PathBuf::from("top.txt"), 1),]);
}

#[tokio::test]
async fn discover_local_tree_lists_directories_parent_before_child() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a/b")).unwrap();

    let tree = discover_tree(&local(), dir.path(), &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(tree.directories, vec![PathBuf::from("a"), PathBuf::from("a/b")]);
}

#[tokio::test]
async fn discover_local_tree_includes_empty_directories() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("empty")).unwrap();

    let tree = discover_tree(&local(), dir.path(), &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(tree.directories, vec![PathBuf::from("empty")]);
    assert!(tree.files.is_empty());
}

#[tokio::test]
async fn discover_local_tree_skips_symlinks_and_counts_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("real.txt"), b"a").unwrap();
    symlink(dir.path().join("real.txt"), dir.path().join("link.txt")).unwrap();

    let tree = discover_tree(&local(), dir.path(), &AtomicBool::new(false)).await.unwrap().unwrap();

    assert_eq!(
        tree.files.into_iter().map(|(path, size, _)| (path, size)).collect::<Vec<_>>(),
        vec![(PathBuf::from("real.txt"), 1)]
    );
    assert_eq!(tree.skipped_symlinks, 1);
}

#[tokio::test]
async fn ensure_local_directory_creates_a_missing_directory() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new");

    ensure_directory(&local(), &target).await.unwrap();

    assert!(target.is_dir());
}

#[tokio::test]
async fn ensure_local_directory_is_a_no_op_when_it_already_exists() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("existing");
    std::fs::create_dir(&target).unwrap();

    ensure_directory(&local(), &target).await.unwrap();

    assert!(target.is_dir());
}

fn sample_entry(name: &str, path: &str, is_dir: bool, size: u64) -> Entry {
    Entry { name: name.to_string(), path: PathBuf::from(path), is_dir, size, permissions: None }
}

#[test]
fn planned_file_for_loose_entry_maps_paths_for_an_upload() {
    let entry = sample_entry("a.txt", "/local/a.txt", false, 10);
    let dest_dir = PathBuf::from("/remote/dest");

    let planned = planned_file_for_loose_entry(&entry, &dest_dir);

    assert_eq!(planned.source, PathBuf::from("/local/a.txt"));
    assert_eq!(planned.destination, PathBuf::from("/remote/dest/a.txt"));
    assert_eq!(planned.display_name, "a.txt");
    assert_eq!(planned.size, 10);
}

#[test]
fn planned_file_for_loose_entry_maps_paths_for_a_download() {
    let entry = sample_entry("a.txt", "/remote/a.txt", false, 10);
    let dest_dir = PathBuf::from("/local/dest");

    let planned = planned_file_for_loose_entry(&entry, &dest_dir);

    assert_eq!(planned.destination, PathBuf::from("/local/dest/a.txt"));
    assert_eq!(planned.source, PathBuf::from("/remote/a.txt"));
    assert_eq!(planned.display_name, "a.txt");
    assert_eq!(planned.size, 10);
}

#[test]
fn planned_files_for_tree_maps_paths_for_an_upload_including_a_nested_file() {
    let entry = sample_entry("myfolder", "/local/myfolder", true, 0);
    let dest_root = PathBuf::from("/remote/dest/myfolder");
    let tree = DiscoveredTree {
        directories: vec![PathBuf::from("sub")],
        files: vec![(PathBuf::from("top.txt"), 5, Some(50)), (PathBuf::from("sub/nested.txt"), 7, None)],
        skipped_symlinks: 0,
    };

    let mut planned = planned_files_for_tree(&entry, &dest_root, &tree);
    planned.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    assert_eq!(planned.len(), 2);

    assert_eq!(planned[0].source, PathBuf::from("/local/myfolder/sub/nested.txt"));
    assert_eq!(planned[0].destination, PathBuf::from("/remote/dest/myfolder/sub/nested.txt"));
    assert_eq!(planned[0].display_name, "sub/nested.txt");
    assert_eq!(planned[0].size, 7);

    assert_eq!(planned[1].source, PathBuf::from("/local/myfolder/top.txt"));
    assert_eq!(planned[1].destination, PathBuf::from("/remote/dest/myfolder/top.txt"));
    assert_eq!(planned[1].display_name, "top.txt");
    assert_eq!(planned[1].size, 5);
}

#[test]
fn planned_files_for_tree_maps_paths_for_a_download_including_a_nested_file() {
    let entry = sample_entry("myfolder", "/remote/myfolder", true, 0);
    let dest_root = PathBuf::from("/local/dest/myfolder");
    let tree = DiscoveredTree {
        directories: vec![PathBuf::from("sub")],
        files: vec![(PathBuf::from("top.txt"), 5, Some(50)), (PathBuf::from("sub/nested.txt"), 7, None)],
        skipped_symlinks: 0,
    };

    let mut planned = planned_files_for_tree(&entry, &dest_root, &tree);
    planned.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    assert_eq!(planned.len(), 2);

    assert_eq!(planned[0].destination, PathBuf::from("/local/dest/myfolder/sub/nested.txt"));
    assert_eq!(planned[0].source, PathBuf::from("/remote/myfolder/sub/nested.txt"));
    assert_eq!(planned[0].display_name, "sub/nested.txt");
    assert_eq!(planned[0].size, 7);

    assert_eq!(planned[1].destination, PathBuf::from("/local/dest/myfolder/top.txt"));
    assert_eq!(planned[1].source, PathBuf::from("/remote/myfolder/top.txt"));
    assert_eq!(planned[1].display_name, "top.txt");
    assert_eq!(planned[1].size, 5);
}

#[tokio::test]
async fn discover_local_tree_returns_none_when_already_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/nested.txt"), b"a").unwrap();

    let tree = discover_tree(&local(), dir.path(), &AtomicBool::new(true)).await.unwrap();

    assert!(tree.is_none());
}

fn planned(source: &str, destination: &str) -> PlannedFile {
    PlannedFile {
        source: PathBuf::from(source),
        destination: PathBuf::from(destination),
        display_name: "x".to_string(),
        size: 1,
        existing: None,
        source_modified: None,
        partial: None,
        resume: false,
    }
}

fn existing(is_dir: bool) -> ExistingFile {
    ExistingFile { size: 7, modified: Some(100), is_dir }
}

#[test]
fn mark_conflicts_flags_files_whose_name_exists_at_the_destination() {
    let mut files = vec![
        planned("/local/a.txt", "/remote/dest/a.txt"),
        planned("/local/b.txt", "/remote/dest/b.txt"),
        planned("/local/c", "/remote/dest/c"),
    ];
    let listing: DestinationListing =
        [("a.txt".to_string(), existing(false)), ("c".to_string(), existing(true))].into_iter().collect();
    let listings: HashMap<PathBuf, DestinationListing> =
        [(PathBuf::from("/remote/dest"), listing)].into_iter().collect();

    mark_conflicts(&mut files, &listings);

    assert_eq!(files[0].existing, Some(existing(false)));
    assert_eq!(files[1].existing, None);
    assert_eq!(files[2].existing, Some(existing(true)));
}

#[test]
fn mark_conflicts_uses_the_local_side_for_downloads_and_treats_unlisted_folders_as_empty() {
    let mut files = vec![planned("/remote/a.txt", "/local/dest/a.txt"), planned("/remote/b.txt", "/local/other/a.txt")];
    let listing: DestinationListing = [("a.txt".to_string(), existing(false))].into_iter().collect();
    let listings: HashMap<PathBuf, DestinationListing> =
        [(PathBuf::from("/local/dest"), listing)].into_iter().collect();

    mark_conflicts(&mut files, &listings);

    assert!(files[0].existing.is_some());
    assert!(files[1].existing.is_none());
}

#[test]
fn parents_created_by_the_scan_are_not_listed() {
    let files = vec![
        planned("/l/a", "/remote/dest/a"),
        planned("/l/b", "/remote/dest/new/b"),
        planned("/l/c", "/remote/dest/new/sub/c"),
        planned("/l/d", "/remote/dest/d"),
    ];
    let created: HashSet<PathBuf> =
        [PathBuf::from("/remote/dest/new"), PathBuf::from("/remote/dest/new/sub")].into_iter().collect();

    assert_eq!(parents_needing_listing(&files, &created), vec![PathBuf::from("/remote/dest")]);
}

#[test]
fn taken_names_combine_existing_and_planned_names_per_folder() {
    let files = vec![planned("/l/a", "/remote/dest/a.txt"), planned("/l/b", "/remote/dest/b.txt")];
    let listing: DestinationListing = [("old.txt".to_string(), existing(false))].into_iter().collect();
    let listings: HashMap<PathBuf, DestinationListing> =
        [(PathBuf::from("/remote/dest"), listing)].into_iter().collect();

    let taken = taken_names(&files, &listings);

    let names = &taken[&PathBuf::from("/remote/dest")];
    assert_eq!(names.len(), 3);
    assert!(names.contains("old.txt") && names.contains("a.txt") && names.contains("b.txt"));
}

#[tokio::test]
async fn list_local_destination_reports_files_and_folders_and_treats_missing_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();

    let listing = list_destination(&local(), dir.path(), &[]).await.unwrap();
    let missing = list_destination(&local(), &dir.path().join("nope"), &[]).await.unwrap();

    assert_eq!(listing["a.txt"].size, 5);
    assert!(!listing["a.txt"].is_dir);
    assert!(listing["a.txt"].modified.is_some());
    assert!(listing["sub"].is_dir);
    assert!(missing.is_empty());
}

#[tokio::test]
async fn ensure_local_directory_reports_whether_it_created_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new");

    assert!(ensure_directory(&local(), &target).await.unwrap());
    assert!(!ensure_directory(&local(), &target).await.unwrap());
}

#[tokio::test]
async fn discover_local_tree_records_modification_times() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"a").unwrap();

    let tree = discover_tree(&local(), dir.path(), &AtomicBool::new(false)).await.unwrap().unwrap();

    assert!(tree.files[0].2.is_some());
}

#[test]
fn planned_files_for_tree_carries_the_source_modification_time() {
    let entry = sample_entry("myfolder", "/local/myfolder", true, 0);
    let tree = DiscoveredTree {
        directories: Vec::new(),
        files: vec![(PathBuf::from("top.txt"), 5, Some(50))],
        skipped_symlinks: 0,
    };

    let planned = planned_files_for_tree(&entry, &PathBuf::from("/remote/dest/myfolder"), &tree);

    assert_eq!(planned[0].source_modified, Some(50));
}

#[tokio::test]
async fn list_local_destination_sees_through_symlinks_and_keeps_dangling_ones() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("real_folder")).unwrap();
    symlink(dir.path().join("real_folder"), dir.path().join("photos")).unwrap();
    symlink(dir.path().join("missing"), dir.path().join("dangling")).unwrap();

    let listing = list_destination(&local(), dir.path(), &[]).await.unwrap();

    assert!(listing["photos"].is_dir);
    assert!(!listing["dangling"].is_dir);
}

#[test]
fn mark_conflicts_finds_a_partial_beside_the_file_and_on_its_own() {
    let mut files = vec![
        planned("/l/a", "/remote/dest/a.iso"),
        planned("/l/b", "/remote/dest/b.iso"),
        planned("/l/c", "/remote/dest/c.iso"),
    ];
    let listing: DestinationListing = [
        ("a.iso".to_string(), existing(false)),
        ("a.iso.part".to_string(), existing(false)),
        ("b.iso.part".to_string(), existing(false)),
    ]
    .into_iter()
    .collect();
    let listings: HashMap<PathBuf, DestinationListing> =
        [(PathBuf::from("/remote/dest"), listing)].into_iter().collect();

    mark_conflicts(&mut files, &listings);

    assert!(files[0].existing.is_some() && files[0].partial.is_some());
    assert!(files[1].existing.is_none() && files[1].partial.is_some());
    assert!(files[2].existing.is_none() && files[2].partial.is_none());
    assert!(files[1].is_conflict());
    assert!(!files[2].is_conflict());
}

#[test]
fn a_folder_named_like_the_partial_is_not_a_partial() {
    let mut files = vec![planned("/l/a", "/remote/dest/a.iso")];
    let listing: DestinationListing = [("a.iso.part".to_string(), existing(true))].into_iter().collect();
    let listings: HashMap<PathBuf, DestinationListing> =
        [(PathBuf::from("/remote/dest"), listing)].into_iter().collect();

    mark_conflicts(&mut files, &listings);

    assert!(files[0].partial.is_none());
}

fn dir_entry(path: &str) -> Entry {
    let path = PathBuf::from(path);
    Entry { name: path.file_name().unwrap().to_string_lossy().into(), path, is_dir: true, size: 0, permissions: None }
}

fn file_entry(path: &str, size: u64) -> Entry {
    let path = PathBuf::from(path);
    Entry { name: path.file_name().unwrap().to_string_lossy().into(), path, is_dir: false, size, permissions: None }
}

fn ready(outcome: Result<PlanOutcome, ProtocolError>) -> DirectoryPlan {
    match outcome.unwrap() {
        PlanOutcome::Ready(plan) => plan,
        PlanOutcome::Cancelled => panic!("unexpected cancel"),
    }
}

#[tokio::test]
async fn a_symlink_inside_a_copied_folder_is_skipped_and_counted() {
    let source = FakeFs::new();
    source.file("/src/photos/a.jpg", b"aa", Some(1)).symlink("/src/photos/latest", "/src/photos/a.jpg");
    let destination = FakeFs::new();
    destination.dir("/dst");

    let plan = ready(
        plan_copy(&source, &destination, vec![dir_entry("/src/photos")], Path::new("/dst"), &AtomicBool::new(false))
            .await,
    );

    assert_eq!(plan.skipped_symlinks, 1);
    assert_eq!(plan.files.iter().map(|file| file.display_name.as_str()).collect::<Vec<_>>(), ["a.jpg"]);
    assert!(destination.exists("/dst/photos"));
}

#[tokio::test]
async fn a_destination_symlink_to_a_file_is_a_conflict_with_the_target_size() {
    let source = FakeFs::new();
    source.file("/src/report.pdf", b"new!", Some(5));
    let destination = FakeFs::new();
    destination.file("/dst/real.pdf", b"old-and-longer", Some(3)).symlink("/dst/report.pdf", "/dst/real.pdf");

    let plan = ready(
        plan_copy(
            &source,
            &destination,
            vec![file_entry("/src/report.pdf", 4)],
            Path::new("/dst"),
            &AtomicBool::new(false),
        )
        .await,
    );

    assert_eq!(plan.files[0].existing.map(|existing| existing.size), Some(14));
}

#[tokio::test]
async fn an_unlistable_destination_falls_back_to_checking_each_planned_name() {
    let source = FakeFs::new();
    source.file("/src/a", b"1", None);
    let destination = FakeFs::new();
    destination.file("/dst/a", b"22", None).fail_read_dir("/dst");

    let plan = ready(
        plan_copy(&source, &destination, vec![file_entry("/src/a", 1)], Path::new("/dst"), &AtomicBool::new(false))
            .await,
    );

    assert_eq!(plan.files[0].existing.map(|existing| existing.size), Some(2));
}

#[tokio::test]
async fn a_leftover_part_file_is_reported_as_a_partial() {
    let source = FakeFs::new();
    source.file("/src/big.iso", b"0123456789", Some(9));
    let destination = FakeFs::new();
    destination.file("/dst/big.iso.part", b"01234", Some(10));

    let plan = ready(
        plan_copy(
            &source,
            &destination,
            vec![file_entry("/src/big.iso", 10)],
            Path::new("/dst"),
            &AtomicBool::new(false),
        )
        .await,
    );

    assert_eq!(plan.files[0].partial.map(|partial| partial.size), Some(5));
    assert_eq!(plan.files[0].source_modified, Some(9));
}

#[tokio::test]
async fn created_destination_folders_are_not_listed_for_conflicts() {
    let source = FakeFs::new();
    source.file("/src/new/x", b"1", None);
    let destination = FakeFs::new();
    destination.dir("/dst");

    let plan = ready(
        plan_copy(&source, &destination, vec![dir_entry("/src/new")], Path::new("/dst"), &AtomicBool::new(false)).await,
    );

    assert!(plan.files[0].existing.is_none());
    assert_eq!(plan.files[0].destination, PathBuf::from("/dst/new/x"));
}

#[tokio::test]
async fn a_download_of_a_remote_folder_plans_every_file_under_the_local_destination() {
    let remote = FakeFs::new();
    remote.file("/srv/site/index.html", b"<html>", Some(4)).file("/srv/site/css/app.css", b"body{}", Some(5));
    let dir = tempfile::tempdir().unwrap();

    let plan =
        ready(plan_copy(&remote, &local(), vec![dir_entry("/srv/site")], dir.path(), &AtomicBool::new(false)).await);

    let mut destinations: Vec<PathBuf> = plan.files.iter().map(|file| file.destination.clone()).collect();
    destinations.sort();
    assert_eq!(destinations, vec![dir.path().join("site/css/app.css"), dir.path().join("site/index.html")]);
    assert!(dir.path().join("site/css").is_dir());
}

#[tokio::test]
async fn planning_that_starts_cancelled_reports_cancelled() {
    let source = FakeFs::new();
    source.file("/src/a", b"1", None);

    let outcome =
        plan_copy(&source, &FakeFs::new(), vec![file_entry("/src/a", 1)], Path::new("/"), &AtomicBool::new(true))
            .await
            .unwrap();

    assert!(matches!(outcome, PlanOutcome::Cancelled));
}
