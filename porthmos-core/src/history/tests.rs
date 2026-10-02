use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use super::{
    testing::{SaveNow, sample},
    *,
};
use crate::Paths;

fn paths() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    (dir, paths)
}

fn labels(history: &History) -> Vec<String> {
    history.entries().iter().map(|entry| entry.label.clone()).collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn broken_file(paths: &Paths) -> PathBuf {
    paths.history_file().with_extension("toml.broken")
}

fn write_history_file(paths: &Paths, contents: impl AsRef<[u8]>) {
    std::fs::create_dir_all(&paths.state_dir).unwrap();
    std::fs::write(paths.history_file(), contents).unwrap();
}

#[test]
fn every_result_and_field_round_trips_through_the_file() {
    let (_dir, paths) = paths();
    let (mut history, warning) = History::load(&paths);
    assert!(warning.is_none());
    let results = [
        HistoryResult::Done,
        HistoryResult::PartlyFailed { failed: 2 },
        HistoryResult::Failed,
        HistoryResult::Cancelled,
        HistoryResult::Interrupted,
    ];
    let mut written = Vec::new();
    for (index, result) in results.into_iter().enumerate() {
        let mut entry = sample(&format!("file{index}"), result);
        entry.direction = if index % 2 == 0 { Direction::Upload } else { Direction::Download };
        entry.files_done = index;
        entry.files_total = index + 3;
        entry.bytes = 1_000_000 + index as u64;
        entry.failed_count = index;
        entry.failed_files = vec![format!("bad{index}.txt")];
        history.record(entry.clone()).unwrap();
        written.push(entry);
    }

    let (reloaded, warning) = History::load(&paths);

    assert!(warning.is_none());
    assert_eq!(reloaded.entries(), written.as_slice());
}

#[test]
fn a_plain_result_is_written_as_a_string_and_a_partial_one_as_a_table() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);
    history.record(sample("a", HistoryResult::Done)).unwrap();
    history.record(sample("b", HistoryResult::PartlyFailed { failed: 2 })).unwrap();

    let text = read(&paths.history_file());

    assert!(text.contains("result = \"done\""), "{text}");
    assert!(text.contains("partly_failed"), "{text}");
    assert!(text.contains("failed = 2"), "{text}");
}

#[test]
fn awkward_text_survives_the_round_trip() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);
    let mut entry = sample("quote\" and\nnewline \u{fc}.txt", HistoryResult::Failed);
    entry.connection = "my \"host\"".to_string();
    entry.failed_files = vec!["tab\there".to_string(), "\u{1f600}".to_string(), "back\\slash".to_string()];
    history.record(entry.clone()).unwrap();

    let (reloaded, warning) = History::load(&paths);

    assert!(warning.is_none());
    assert_eq!(reloaded.entries(), [entry].as_slice());
}

#[test]
fn entries_with_the_same_time_keep_their_recording_order() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);
    for label in ["a", "b", "c"] {
        history.record(sample(label, HistoryResult::Done)).unwrap();
    }

    assert_eq!(labels(&history), ["a", "b", "c"]);
    let newest: Vec<String> = history.newest_first().into_iter().map(|entry| entry.label).collect();
    assert_eq!(newest, ["c", "b", "a"]);
}

#[test]
fn recording_past_the_cap_drops_the_oldest_entry() {
    let (_dir, paths) = paths();
    let entries: Vec<_> = (0..MAX_ENTRIES).map(|index| sample(&format!("e{index}"), HistoryResult::Done)).collect();
    let mut history = History { path: paths.history_file(), entries: Arc::new(entries), writable: true };

    history.record(sample("new", HistoryResult::Done)).unwrap();

    assert_eq!(history.entries().len(), MAX_ENTRIES);
    assert_eq!(history.entries().first().unwrap().label, "e1");
    assert_eq!(history.entries().last().unwrap().label, "new");
    let (reloaded, _) = History::load(&paths);
    assert_eq!(reloaded.entries().len(), MAX_ENTRIES);
    assert_eq!(reloaded.entries().first().unwrap().label, "e1");
}

#[test]
fn loading_a_file_with_too_many_entries_keeps_the_newest() {
    let (_dir, paths) = paths();
    let entries = (0..MAX_ENTRIES + 5).map(|index| sample(&format!("e{index}"), HistoryResult::Done)).collect();
    History { path: paths.history_file(), entries: Arc::new(entries), writable: true }.save_now().unwrap();

    let (history, warning) = History::load(&paths);

    assert!(warning.is_none());
    assert_eq!(history.entries().len(), MAX_ENTRIES);
    assert_eq!(history.entries().first().unwrap().label, "e5");
    assert_eq!(history.entries().last().unwrap().label, format!("e{}", MAX_ENTRIES + 4));
}

#[test]
fn the_file_is_private_and_no_temporary_file_is_left() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);

    history.record(sample("a", HistoryResult::Done)).unwrap();
    assert_eq!(mode(&paths.history_file()), 0o600);
    history.record(sample("b", HistoryResult::Done)).unwrap();
    assert_eq!(mode(&paths.history_file()), 0o600);

    assert!(!paths.history_file().with_extension("toml.tmp").exists());
}

#[test]
fn a_missing_file_is_an_empty_history_without_a_message_or_a_file() {
    let (_dir, paths) = paths();

    let (history, warning) = History::load(&paths);

    assert!(history.entries().is_empty());
    assert!(warning.is_none());
    assert!(!paths.history_file().exists());
}

#[test]
fn an_unparsable_file_is_set_aside_with_a_warning_and_never_overwritten() {
    let (_dir, paths) = paths();
    write_history_file(&paths, "not toml [");

    let (mut history, warning) = History::load(&paths);

    assert!(history.entries().is_empty());
    let warning = warning.unwrap();
    assert!(warning.contains("history.toml.broken"), "{warning}");
    assert_eq!(read(&broken_file(&paths)), "not toml [");
    assert!(!paths.history_file().exists());

    history.record(sample("a", HistoryResult::Done)).unwrap();

    assert_eq!(read(&broken_file(&paths)), "not toml [");
    assert_eq!(labels(&History::load(&paths).0), ["a"]);
}

#[test]
fn a_file_of_the_wrong_shape_is_set_aside() {
    let (_dir, paths) = paths();
    write_history_file(&paths, "entries = \"nothing\"\n");

    let (history, warning) = History::load(&paths);

    assert!(history.entries().is_empty());
    assert!(warning.is_some());
    assert!(broken_file(&paths).exists());
}

#[test]
fn an_entry_missing_a_required_field_sets_the_whole_file_aside() {
    let (_dir, paths) = paths();
    write_history_file(&paths, "[[entries]]\nconnection = \"c\"\n");

    let (history, warning) = History::load(&paths);

    assert!(history.entries().is_empty());
    assert!(warning.is_some());
    assert!(broken_file(&paths).exists());
}

#[test]
fn a_file_that_is_not_utf8_is_set_aside_byte_for_byte() {
    let (_dir, paths) = paths();
    write_history_file(&paths, [0xff, 0xfe, 0x00]);

    let (history, warning) = History::load(&paths);

    assert!(history.entries().is_empty());
    assert!(warning.is_some());
    assert_eq!(std::fs::read(broken_file(&paths)).unwrap(), vec![0xff, 0xfe, 0x00]);
}

#[test]
fn a_second_broken_file_never_replaces_the_first_set_aside() {
    let (_dir, paths) = paths();
    write_history_file(&paths, "first [");
    History::load(&paths);
    write_history_file(&paths, "second [");

    History::load(&paths);

    assert_eq!(read(&broken_file(&paths)), "first [");
    let mut second = broken_file(&paths).into_os_string();
    second.push(".1");
    assert_eq!(read(&PathBuf::from(second)), "second [");
}

#[test]
fn optional_fields_may_be_missing_from_an_older_file() {
    let (_dir, paths) = paths();
    write_history_file(
        &paths,
        "[[entries]]\nfinished_at = \"2026-09-30T08:12:44Z\"\nconnection = \"c\"\ndirection = \"upload\"\n\
         label = \"l\"\nlocal_path = \"/l\"\nremote_path = \"/r\"\nfiles_done = 1\nfiles_total = 1\nbytes = 5\n\
         result = \"done\"\n",
    );

    let (history, warning) = History::load(&paths);

    assert!(warning.is_none());
    assert_eq!(history.entries().len(), 1);
    assert_eq!(history.entries()[0].failed_count, 0);
    assert!(history.entries()[0].failed_files.is_empty());
}

#[test]
fn an_unreadable_location_warns_and_starts_empty() {
    let (_dir, paths) = paths();
    std::fs::write(&paths.state_dir, b"a file where the directory should be").unwrap();

    let (history, warning) = History::load(&paths);

    assert!(history.entries().is_empty());
    assert!(warning.unwrap().contains("Couldn't read transfer history"));
}

#[test]
fn a_failed_write_returns_the_error_and_keeps_the_entry_in_memory() {
    let (_dir, paths) = paths();
    std::fs::write(&paths.state_dir, b"a file where the directory should be").unwrap();
    let (mut history, _) = History::load(&paths);

    let error = history.record(sample("a", HistoryResult::Done)).unwrap_err();

    assert!(!format!("{error:#}").is_empty());
    assert_eq!(labels(&history), ["a"]);
}

#[test]
fn clear_empties_memory_and_the_file() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);
    history.record(sample("a", HistoryResult::Done)).unwrap();
    history.record(sample("b", HistoryResult::Done)).unwrap();

    history.clear().unwrap();

    assert!(history.entries().is_empty());
    let (reloaded, warning) = History::load(&paths);
    assert!(reloaded.entries().is_empty());
    assert!(warning.is_none());
}

#[test]
fn a_failed_clear_still_empties_memory_and_reports_the_error() {
    let (_dir, paths) = paths();
    std::fs::write(&paths.state_dir, b"a file where the directory should be").unwrap();
    let mut history = History {
        path: paths.history_file(),
        entries: Arc::new(vec![sample("a", HistoryResult::Done)]),
        writable: true,
    };

    assert!(history.clear().is_err());

    assert!(history.entries().is_empty());
}

#[test]
fn result_text_is_stable() {
    assert_eq!(HistoryResult::Done.text(), "done");
    assert_eq!(HistoryResult::PartlyFailed { failed: 3 }.text(), "partly failed");
    assert_eq!(HistoryResult::Failed.text(), "failed");
    assert_eq!(HistoryResult::Cancelled.text(), "cancelled");
    assert_eq!(HistoryResult::Interrupted.text(), "interrupted");
}

fn report() -> HistoryEntry {
    let mut entry = sample("Report.PDF", HistoryResult::PartlyFailed { failed: 1 });
    entry.connection = "Prod-Box".to_string();
    entry.local_path = "/home/me/docs/Report.PDF".to_string();
    entry.remote_path = "/srv/in/Report.PDF".to_string();
    entry
}

#[test]
fn plain_text_matches_any_field_ignoring_case() {
    let entry = report();

    for filter in ["prod", "REPORT", "docs", "/srv/in", "partly", "failed"] {
        assert!(matches(&entry, filter), "{filter}");
    }
    assert!(!matches(&entry, "nomatch"));
}

#[test]
fn failed_matches_partly_failed_but_not_done() {
    assert!(matches(&sample("a", HistoryResult::Failed), "failed"));
    assert!(matches(&sample("a", HistoryResult::PartlyFailed { failed: 1 }), "failed"));
    assert!(!matches(&sample("a", HistoryResult::Done), "failed"));
}

#[test]
fn a_glob_is_anchored_to_a_whole_field() {
    let entry = report();

    assert!(matches(&entry, "rep*"));
    assert!(matches(&entry, "*.pdf"));
    assert!(matches(&entry, "prod-bo?"));
    assert!(!matches(&entry, "port*"));
}

#[test]
fn an_empty_or_blank_filter_matches_everything() {
    let entry = report();

    assert!(matches(&entry, ""));
    assert!(matches(&entry, "   "));
}

fn can_still_read(path: &Path) -> bool {
    std::fs::read(path).is_ok()
}

#[test]
fn a_file_that_cannot_be_read_is_set_aside_untouched_and_not_overwritten() {
    let (_dir, paths) = paths();
    let (mut first, _) = History::load(&paths);
    first.record(sample("precious", HistoryResult::Done)).unwrap();
    let original = std::fs::read(paths.history_file()).unwrap();
    std::fs::set_permissions(paths.history_file(), std::fs::Permissions::from_mode(0o000)).unwrap();
    if can_still_read(&paths.history_file()) {
        return;
    }

    let (mut history, warning) = History::load(&paths);
    history.record(sample("new", HistoryResult::Done)).unwrap();

    assert!(warning.unwrap().contains("Couldn't read transfer history"));
    std::fs::set_permissions(broken_file(&paths), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(broken_file(&paths)).unwrap(), original);
    assert_eq!(labels(&History::load(&paths).0), ["new"]);
}

#[test]
fn when_the_file_cannot_be_set_aside_nothing_is_ever_written_over_it() {
    let (_dir, paths) = paths();
    write_history_file(&paths, "not toml [");
    std::fs::set_permissions(&paths.state_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::File::create(paths.state_dir.join("probe")).is_ok() {
        std::fs::set_permissions(&paths.state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }

    let (mut history, warning) = History::load(&paths);
    std::fs::set_permissions(&paths.state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = history.record(sample("a", HistoryResult::Done));

    let warning = warning.unwrap();
    assert!(warning.contains("couldn't be set aside"), "{warning}");
    assert!(result.is_err());
    assert_eq!(labels(&history), ["a"]);
    assert_eq!(read(&paths.history_file()), "not toml [");
}

#[test]
fn push_and_render_do_not_touch_the_disk_and_render_matches_what_record_writes() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);

    history.push(sample("a", HistoryResult::Done));
    let rendered = history.render().unwrap();

    assert!(!paths.history_file().exists());
    history.record(sample("b", HistoryResult::Done)).unwrap();
    assert_ne!(rendered, read(&paths.history_file()));
    assert!(read(&paths.history_file()).contains("label = \"a\""));
}

#[test]
fn clear_memory_empties_the_entries_without_writing() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);
    history.record(sample("a", HistoryResult::Done)).unwrap();

    history.clear_memory();

    assert!(history.entries().is_empty());
    assert!(read(&paths.history_file()).contains("label = \"a\""));
}

#[test]
fn a_protected_history_reports_it_cannot_be_written() {
    let (_dir, paths) = paths();
    std::fs::write(&paths.state_dir, b"a file where the directory should be").unwrap();

    let (history, _) = History::load(&paths);

    assert!(!history.is_writable());
    assert_eq!(history.path(), paths.history_file());
}

#[test]
fn a_snapshot_renders_exactly_what_render_does() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);
    history.push(sample("a", HistoryResult::Done));
    history.push(sample("b", HistoryResult::Failed));

    let rendered = render_entries(&history.snapshot()).unwrap();

    assert_eq!(rendered, history.render().unwrap());
}

#[test]
fn a_snapshot_is_not_changed_by_later_entries() {
    let (_dir, paths) = paths();
    let (mut history, _) = History::load(&paths);
    history.push(sample("a", HistoryResult::Done));
    let snapshot = history.snapshot();

    history.push(sample("b", HistoryResult::Done));
    history.clear_memory();

    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].label, "a");
}
