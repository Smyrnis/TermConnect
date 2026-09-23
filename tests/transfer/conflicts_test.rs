use std::{collections::HashMap, path::PathBuf};

use super::*;
use crate::transfer::{
    Direction,
    plan::{DirectoryPlan, ExistingFile, PlannedFile},
};

fn taken(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn unique_name_inserts_a_counter_before_the_extension() {
    assert_eq!(unique_name("report.pdf", &taken(&["report.pdf"])), "report (1).pdf");
}

#[test]
fn unique_name_skips_counters_that_are_already_taken() {
    assert_eq!(unique_name("report.pdf", &taken(&["report.pdf", "report (1).pdf", "report (2).pdf"])), "report (3).pdf");
}

#[test]
fn unique_name_handles_names_without_an_extension_and_dotfiles() {
    assert_eq!(unique_name("README", &taken(&["README"])), "README (1)");
    assert_eq!(unique_name(".env", &taken(&[".env"])), ".env (1)");
    assert_eq!(unique_name("backup.tar.gz", &taken(&["backup.tar.gz"])), "backup.tar (1).gz");
}

#[test]
fn only_ask_has_no_automatic_resolution() {
    assert_eq!(ConflictPolicy::Ask.automatic_resolution(), None);
    assert_eq!(ConflictPolicy::Overwrite.automatic_resolution(), Some(Resolution::Overwrite));
    assert_eq!(ConflictPolicy::Skip.automatic_resolution(), Some(Resolution::Skip));
    assert_eq!(ConflictPolicy::Rename.automatic_resolution(), Some(Resolution::Rename));
}

fn upload(name: &str, conflict: Option<bool>) -> PlannedFile {
    PlannedFile { local_path: PathBuf::from(format!("/local/{name}")), remote_path: format!("/remote/dest/{name}"), display_name: format!("sub/{name}"), size: 1, existing: conflict.map(|is_dir| ExistingFile { size: 2, modified: None, is_dir }), source_modified: None }
}

fn plan(files: Vec<PlannedFile>) -> DirectoryPlan {
    let names: HashSet<String> = files.iter().map(|file| file.remote_path.rsplit('/').next().unwrap().to_string()).collect();
    DirectoryPlan { files, skipped_symlinks: 0, taken_names: HashMap::from([(PathBuf::from("/remote/dest"), names)]) }
}

#[test]
fn conflict_indices_lists_only_conflicting_files() {
    let plan = plan(vec![upload("a", None), upload("b", Some(false)), upload("c", Some(true))]);

    assert_eq!(conflict_indices(&plan), vec![1, 2]);
}

#[test]
fn resolve_applies_each_answer_in_order_and_passes_other_files_through() {
    let plan = plan(vec![upload("a.txt", None), upload("b.txt", Some(false)), upload("c.txt", Some(false)), upload("d.txt", Some(false))]);

    let files = resolve(plan, &[Resolution::Overwrite, Resolution::Skip, Resolution::Rename], Direction::Upload).files;

    let remote: Vec<&str> = files.iter().map(|file| file.remote_path.as_str()).collect();
    assert_eq!(remote, vec!["/remote/dest/a.txt", "/remote/dest/b.txt", "/remote/dest/d (1).txt"]);
    assert_eq!(files[2].display_name, "sub/d (1).txt");
}

#[test]
fn two_renames_in_one_folder_get_different_names() {
    let mut first = upload("a.txt", Some(false));
    let mut second = upload("a.txt", Some(false));
    second.local_path = PathBuf::from("/local/other/a.txt");
    first.display_name = "a.txt".to_string();
    second.display_name = "other/a.txt".to_string();

    let files = resolve(plan(vec![first, second]), &[Resolution::Rename, Resolution::Rename], Direction::Upload).files;

    assert_eq!(files[0].remote_path, "/remote/dest/a (1).txt");
    assert_eq!(files[1].remote_path, "/remote/dest/a (2).txt");
}

#[test]
fn an_existing_folder_is_never_overwritten() {
    let files = resolve(plan(vec![upload("photos", Some(true))]), &[Resolution::Overwrite], Direction::Upload).files;

    assert!(files.is_empty());
}

#[test]
fn a_missing_answer_skips_the_file() {
    let files = resolve(plan(vec![upload("a.txt", Some(false))]), &[], Direction::Upload).files;

    assert!(files.is_empty());
}

#[test]
fn rename_rewrites_the_local_path_for_downloads() {
    let file = PlannedFile { local_path: PathBuf::from("/local/dest/a.txt"), remote_path: "/remote/a.txt".to_string(), display_name: "a.txt".to_string(), size: 1, existing: Some(ExistingFile { size: 2, modified: None, is_dir: false }), source_modified: None };
    let plan = DirectoryPlan { files: vec![file], skipped_symlinks: 0, taken_names: HashMap::from([(PathBuf::from("/local/dest"), HashSet::from(["a.txt".to_string()]))]) };

    let files = resolve(plan, &[Resolution::Rename], Direction::Download).files;

    assert_eq!(files[0].local_path, PathBuf::from("/local/dest/a (1).txt"));
    assert_eq!(files[0].remote_path, "/remote/a.txt");
}

#[test]
fn resolve_counts_skipped_files_and_files_blocked_by_a_folder() {
    let plan = plan(vec![upload("a.txt", Some(false)), upload("photos", Some(true)), upload("c.txt", Some(false))]);

    let resolved = resolve(plan, &[Resolution::Skip, Resolution::Overwrite, Resolution::Overwrite], Direction::Upload);

    assert_eq!(resolved.files.len(), 1);
    assert_eq!(resolved.skipped, 1);
    assert_eq!(resolved.blocked_by_folder, 1);
}

#[test]
fn rename_rewrites_the_remote_path_and_display_name_of_a_nested_upload() {
    let file = PlannedFile { local_path: PathBuf::from("/local/photos/sub/a.txt"), remote_path: "/remote/dest/photos/sub/a.txt".to_string(), display_name: "sub/a.txt".to_string(), size: 1, existing: Some(ExistingFile { size: 2, modified: None, is_dir: false }), source_modified: None };
    let plan = DirectoryPlan { files: vec![file], skipped_symlinks: 0, taken_names: HashMap::from([(PathBuf::from("/remote/dest/photos/sub"), HashSet::from(["a.txt".to_string()]))]) };

    let files = resolve(plan, &[Resolution::Rename], Direction::Upload).files;

    assert_eq!(files[0].remote_path, "/remote/dest/photos/sub/a (1).txt");
    assert_eq!(files[0].display_name, "sub/a (1).txt");
    assert_eq!(files[0].local_path, PathBuf::from("/local/photos/sub/a.txt"));
}
