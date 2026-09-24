use std::{collections::HashMap, path::PathBuf};

use super::*;
use crate::transfer::plan::{DirectoryPlan, ExistingFile, PlannedFile};

fn taken(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn unique_name_inserts_a_counter_before_the_extension() {
    assert_eq!(unique_name("report.pdf", &taken(&["report.pdf"])), "report (1).pdf");
}

#[test]
fn unique_name_skips_counters_that_are_already_taken() {
    assert_eq!(
        unique_name("report.pdf", &taken(&["report.pdf", "report (1).pdf", "report (2).pdf"])),
        "report (3).pdf"
    );
}

#[test]
fn unique_name_handles_names_without_an_extension_and_dotfiles() {
    assert_eq!(unique_name("README", &taken(&["README"])), "README (1)");
    assert_eq!(unique_name(".env", &taken(&[".env"])), ".env (1)");
    assert_eq!(unique_name("backup.tar.gz", &taken(&["backup.tar.gz"])), "backup.tar (1).gz");
}

fn upload(name: &str, conflict: Option<bool>) -> PlannedFile {
    PlannedFile {
        source: PathBuf::from(format!("/local/{name}")),
        destination: PathBuf::from(format!("/remote/dest/{name}")),
        display_name: format!("sub/{name}"),
        size: 1,
        existing: conflict.map(|is_dir| ExistingFile { size: 2, modified: None, is_dir }),
        source_modified: None,
        partial: None,
        resume: false,
    }
}

fn plan(files: Vec<PlannedFile>) -> DirectoryPlan {
    let names: HashSet<String> =
        files.iter().map(|file| file.destination.file_name().unwrap().to_string_lossy().into_owned()).collect();
    DirectoryPlan { files, skipped_symlinks: 0, taken_names: HashMap::from([(PathBuf::from("/remote/dest"), names)]) }
}

#[test]
fn conflict_indices_lists_only_conflicting_files() {
    let plan = plan(vec![upload("a", None), upload("b", Some(false)), upload("c", Some(true))]);

    assert_eq!(conflict_indices(&plan), vec![1, 2]);
}

#[test]
fn resolve_applies_each_answer_in_order_and_passes_other_files_through() {
    let plan = plan(vec![
        upload("a.txt", None),
        upload("b.txt", Some(false)),
        upload("c.txt", Some(false)),
        upload("d.txt", Some(false)),
    ]);

    let files = resolve(plan, &[Resolution::Overwrite, Resolution::Skip, Resolution::Rename]).files;

    let remote: Vec<&str> = files.iter().map(|file| file.destination.to_str().unwrap()).collect();
    assert_eq!(remote, vec!["/remote/dest/a.txt", "/remote/dest/b.txt", "/remote/dest/d (1).txt"]);
    assert_eq!(files[2].display_name, "sub/d (1).txt");
}

#[test]
fn two_renames_in_one_folder_get_different_names() {
    let mut first = upload("a.txt", Some(false));
    let mut second = upload("a.txt", Some(false));
    second.source = PathBuf::from("/local/other/a.txt");
    first.display_name = "a.txt".to_string();
    second.display_name = "other/a.txt".to_string();

    let files = resolve(plan(vec![first, second]), &[Resolution::Rename, Resolution::Rename]).files;

    assert_eq!(files[0].destination, PathBuf::from("/remote/dest/a (1).txt"));
    assert_eq!(files[1].destination, PathBuf::from("/remote/dest/a (2).txt"));
}

#[test]
fn an_existing_folder_is_never_overwritten() {
    let files = resolve(plan(vec![upload("photos", Some(true))]), &[Resolution::Overwrite]).files;

    assert!(files.is_empty());
}

#[test]
fn a_missing_answer_skips_the_file() {
    let files = resolve(plan(vec![upload("a.txt", Some(false))]), &[]).files;

    assert!(files.is_empty());
}

#[test]
fn rename_rewrites_the_local_path_for_downloads() {
    let file = PlannedFile {
        source: PathBuf::from("/remote/a.txt"),
        destination: PathBuf::from("/local/dest/a.txt"),
        display_name: "a.txt".to_string(),
        size: 1,
        existing: Some(ExistingFile { size: 2, modified: None, is_dir: false }),
        source_modified: None,
        partial: None,
        resume: false,
    };
    let plan = DirectoryPlan {
        files: vec![file],
        skipped_symlinks: 0,
        taken_names: HashMap::from([(PathBuf::from("/local/dest"), HashSet::from(["a.txt".to_string()]))]),
    };

    let files = resolve(plan, &[Resolution::Rename]).files;

    assert_eq!(files[0].destination, PathBuf::from("/local/dest/a (1).txt"));
    assert_eq!(files[0].source, PathBuf::from("/remote/a.txt"));
}

#[test]
fn resolve_counts_skipped_files_and_files_blocked_by_a_folder() {
    let plan = plan(vec![upload("a.txt", Some(false)), upload("photos", Some(true)), upload("c.txt", Some(false))]);

    let resolved = resolve(plan, &[Resolution::Skip, Resolution::Overwrite, Resolution::Overwrite]);

    assert_eq!(resolved.files.len(), 1);
    assert_eq!(resolved.skipped, 1);
    assert_eq!(resolved.blocked_by_folder, 1);
}

#[test]
fn rename_rewrites_the_remote_path_and_display_name_of_a_nested_upload() {
    let file = PlannedFile {
        source: PathBuf::from("/local/photos/sub/a.txt"),
        destination: PathBuf::from("/remote/dest/photos/sub/a.txt"),
        display_name: "sub/a.txt".to_string(),
        size: 1,
        existing: Some(ExistingFile { size: 2, modified: None, is_dir: false }),
        source_modified: None,
        partial: None,
        resume: false,
    };
    let plan = DirectoryPlan {
        files: vec![file],
        skipped_symlinks: 0,
        taken_names: HashMap::from([(PathBuf::from("/remote/dest/photos/sub"), HashSet::from(["a.txt".to_string()]))]),
    };

    let files = resolve(plan, &[Resolution::Rename]).files;

    assert_eq!(files[0].destination, PathBuf::from("/remote/dest/photos/sub/a (1).txt"));
    assert_eq!(files[0].display_name, "sub/a (1).txt");
    assert_eq!(files[0].source, PathBuf::from("/local/photos/sub/a.txt"));
}

#[test]
fn a_file_with_only_a_partial_is_a_conflict() {
    let mut partial_only = upload("a.iso", None);
    partial_only.partial = Some(ExistingFile { size: 5, modified: None, is_dir: false });

    assert_eq!(conflict_indices(&plan(vec![upload("b", None), partial_only])), vec![1]);
}

fn with(existing: Option<bool>, partial: bool) -> PlannedFile {
    let mut file = upload("a.iso", existing);
    file.partial = partial.then_some(ExistingFile { size: 0, modified: None, is_dir: false });
    file
}

#[test]
fn resolution_for_follows_the_policy_and_resumes_partials() {
    assert_eq!(ConflictPolicy::Ask.resolution_for(&with(Some(false), true)), None);
    for policy in [ConflictPolicy::Overwrite, ConflictPolicy::Skip, ConflictPolicy::Rename] {
        assert_eq!(policy.resolution_for(&with(None, true)), Some(Resolution::Resume));
    }
    assert_eq!(ConflictPolicy::Overwrite.resolution_for(&with(Some(false), true)), Some(Resolution::Resume));
    assert_eq!(ConflictPolicy::Overwrite.resolution_for(&with(Some(false), false)), Some(Resolution::Overwrite));
    assert_eq!(ConflictPolicy::Skip.resolution_for(&with(Some(false), true)), Some(Resolution::Skip));
    assert_eq!(ConflictPolicy::Rename.resolution_for(&with(Some(false), true)), Some(Resolution::Rename));
}

#[test]
fn resume_sets_the_flag_and_overwrite_clears_it() {
    let files = resolve(
        plan(vec![with(Some(false), true), with(Some(false), true)]),
        &[Resolution::Resume, Resolution::Overwrite],
    )
    .files;

    assert!(files[0].resume);
    assert!(!files[1].resume);
}

#[test]
fn a_folder_blocks_resume() {
    let resolved = resolve(plan(vec![with(Some(true), true)]), &[Resolution::Resume]);

    assert!(resolved.files.is_empty());
    assert_eq!(resolved.blocked_by_folder, 1);
}

#[test]
fn rename_on_a_partial_only_file_resumes() {
    let files = resolve(plan(vec![with(None, true)]), &[Resolution::Rename]).files;

    assert_eq!(files[0].destination, PathBuf::from("/remote/dest/a.iso"));
    assert!(files[0].resume);
}

#[test]
fn skip_on_a_partial_queues_nothing() {
    let resolved = resolve(plan(vec![with(None, true)]), &[Resolution::Skip]);

    assert!(resolved.files.is_empty());
    assert_eq!(resolved.skipped, 0);
    assert_eq!(resolved.skipped_partials, 1);
}

#[test]
fn an_automatic_policy_starts_over_when_the_partial_is_as_large_as_the_source() {
    let mut file = with(None, true);
    file.partial = Some(ExistingFile { size: file.size, modified: None, is_dir: false });

    assert_eq!(ConflictPolicy::Skip.resolution_for(&file), Some(Resolution::Overwrite));
}

#[test]
fn a_same_answer_only_carries_to_files_it_fits() {
    let partial = with(None, true);
    let complete = with(Some(false), false);
    let both = with(Some(false), true);
    let folder_and_partial = with(Some(true), true);

    assert!(fits_the_rest(Resolution::Resume, &partial, &both));
    assert!(!fits_the_rest(Resolution::Resume, &partial, &complete));
    assert!(!fits_the_rest(Resolution::Resume, &partial, &folder_and_partial));
    assert!(!fits_the_rest(Resolution::Overwrite, &partial, &complete));
    assert!(fits_the_rest(Resolution::Overwrite, &partial, &with(None, true)));
    assert!(fits_the_rest(Resolution::Overwrite, &complete, &both));
    assert!(!fits_the_rest(Resolution::Rename, &complete, &partial));
    assert!(fits_the_rest(Resolution::Skip, &complete, &partial));
}

#[test]
fn conflict_info_carries_what_the_prompt_shows() {
    let mut planned = upload("a.txt", Some(false));
    planned.source_modified = Some(9);

    let info = ConflictInfo::from(&planned);

    assert_eq!(info.display_name, "sub/a.txt");
    assert_eq!(info.existing, planned.existing);
    assert_eq!((info.new_size, info.new_modified), (1, Some(9)));
}

#[test]
fn fits_the_rest_gives_the_same_answer_for_conflict_infos_as_for_planned_files() {
    let complete = upload("a.txt", Some(false));
    let mut partial = upload("b.iso", None);
    partial.partial = Some(ExistingFile { size: 1, modified: None, is_dir: false });

    for resolution in [Resolution::Overwrite, Resolution::Skip, Resolution::Rename, Resolution::Resume] {
        assert_eq!(
            fits_the_rest(resolution, &ConflictInfo::from(&partial), &ConflictInfo::from(&complete)),
            fits_the_rest(resolution, &partial, &complete)
        );
    }
}
