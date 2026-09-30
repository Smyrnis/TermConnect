use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use chrono::Utc;
use porthmos_vfs::{FileSystem, testing::FakeFs};

use super::*;
use crate::{
    edit::{EditChoice, EditQuestionKind, EditorCommand, EditorExit, MAX_EDIT_BYTES},
    engine::{
        Command,
        testing::{TestEngine, test_engine},
    },
};

const REMOTE: &str = "/home/user/a.txt";

fn local_file(t: &TestEngine, name: &str) -> PathBuf {
    let path = t.dir.path().join(name);
    std::fs::write(&path, b"local text").unwrap();
    path
}

fn find_ready(events: &[Event]) -> Option<(u64, PathBuf, EditorCommand)> {
    events.iter().find_map(|event| match event {
        Event::EditReady { edit_id, file, editor } => Some((*edit_id, file.clone(), editor.clone())),
        _ => None,
    })
}

fn notices_of(events: &[Event]) -> Vec<(Severity, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Notice { severity, message } => Some((*severity, message.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_local_file_is_edited_in_place_without_a_temporary_copy() {
    let mut t = test_engine();
    let path = local_file(&t, "l.txt");

    t.engine.handle_command(Command::EditFile { location: Location::Local, path: path.clone() });

    let events = t.drain();
    let (_, file, _) = find_ready(&events).expect("EditReady");
    assert_eq!(file, path);
    assert!(!t.engine.paths.edit_dir().exists());
}

#[test]
fn each_edit_gets_its_own_id() {
    let mut t = test_engine();
    let path = local_file(&t, "l.txt");

    t.engine.handle_command(Command::EditFile { location: Location::Local, path: path.clone() });
    t.engine.handle_command(Command::EditFile { location: Location::Local, path });

    let ids: Vec<u64> = t
        .drain()
        .iter()
        .filter_map(|event| match event {
            Event::EditReady { edit_id, .. } => Some(*edit_id),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn a_folder_is_refused() {
    let mut t = test_engine();

    t.engine.handle_command(Command::EditFile { location: Location::Local, path: t.dir.path().to_path_buf() });

    let events = t.drain();
    assert!(find_ready(&events).is_none());
    assert!(notices_of(&events).iter().any(|(_, message)| message == "Can't edit a folder"));
}

#[test]
fn the_editor_comes_from_the_setting_then_the_environment() {
    let mut t = test_engine();
    let path = local_file(&t, "l.txt");
    t.engine.env.editor = Some("nano -w".to_string());

    t.engine.handle_command(Command::EditFile { location: Location::Local, path: path.clone() });
    let (_, _, from_env) = find_ready(&t.drain()).unwrap();
    t.engine.edit.settings.editor = Some("code --wait".to_string());
    t.engine.handle_command(Command::EditFile { location: Location::Local, path });
    let (_, _, from_setting) = find_ready(&t.drain()).unwrap();

    assert_eq!(from_env, EditorCommand { program: "nano".to_string(), args: vec!["-w".to_string()] });
    assert_eq!(from_setting, EditorCommand { program: "code".to_string(), args: vec!["--wait".to_string()] });
}

#[test]
fn an_unusable_editor_command_is_reported_and_nothing_starts() {
    let mut t = test_engine();
    let path = local_file(&t, "l.txt");
    t.engine.edit.settings.editor = Some("ed \"oops".to_string());

    t.engine.handle_command(Command::EditFile { location: Location::Local, path });

    let events = t.drain();
    assert!(find_ready(&events).is_none());
    let notices = notices_of(&events);
    assert!(
        notices.iter().any(|(severity, message)| *severity == Severity::Error
            && message.starts_with("Couldn't start the editor")
            && message.contains("unterminated quote")),
        "{notices:?}"
    );
}

#[test]
fn finishing_a_local_edit_refreshes_the_local_panel() {
    let mut t = test_engine();
    let path = local_file(&t, "l.txt");
    t.engine.handle_command(Command::EditFile { location: Location::Local, path });
    let (edit_id, _, _) = find_ready(&t.drain()).unwrap();

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Success });

    assert!(t.drain().iter().any(|event| matches!(event, Event::LocationChanged { location: Location::Local })));
    assert!(t.engine.edit.sessions.is_empty());
}

#[test]
fn a_failing_local_editor_only_notifies_and_still_refreshes() {
    let mut t = test_engine();
    let path = local_file(&t, "l.txt");
    t.engine.handle_command(Command::EditFile { location: Location::Local, path });
    let (edit_id, _, _) = find_ready(&t.drain()).unwrap();

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Status(2) });

    let events = t.drain();
    assert!(notices_of(&events).contains(&(Severity::Warning, "Editor exited with status 2".to_string())));
    assert!(events.iter().any(|event| matches!(event, Event::LocationChanged { location: Location::Local })));
}

#[test]
fn a_local_editor_that_could_not_start_is_an_error() {
    let mut t = test_engine();
    let path = local_file(&t, "l.txt");
    t.engine.handle_command(Command::EditFile { location: Location::Local, path });
    let (edit_id, _, _) = find_ready(&t.drain()).unwrap();

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::LaunchFailed("not found".to_string()) });

    let events = t.drain();
    let notices = notices_of(&events);
    assert!(
        notices.iter().any(|(severity, message)| *severity == Severity::Error
            && message == "Couldn't start the editor (not found). Set $EDITOR or [edit] editor"),
        "{notices:?}"
    );
    assert!(t.engine.edit.sessions.is_empty());
}

#[test]
fn commands_for_an_unknown_edit_are_ignored() {
    let mut t = test_engine();

    t.engine.handle_command(Command::FinishEdit { edit_id: 99, exit: EditorExit::Success });
    t.engine.handle_command(Command::ResolveEdit { edit_id: 99, choice: crate::edit::EditChoice::Upload });

    assert!(t.drain().is_empty());
}

fn remote_session(t: &mut TestEngine) -> (u64, FakeFs) {
    let (session, fs) = t.add_session("prod");
    fs.file(REMOTE, b"old content", Some(10));
    (session, fs)
}

async fn open_remote(t: &mut TestEngine, session: u64, path: &str) -> Vec<Event> {
    t.engine.handle_command(Command::EditFile { location: Location::Session(session), path: PathBuf::from(path) });
    t.run_internal().await;
    t.drain()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn error_notices(events: &[Event]) -> Vec<String> {
    notices_of(events)
        .into_iter()
        .filter(|(severity, _)| *severity == Severity::Error)
        .map(|(_, message)| message)
        .collect()
}

#[tokio::test]
async fn a_remote_file_is_downloaded_to_a_private_temporary_copy() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);

    let events = open_remote(&mut t, session, REMOTE).await;

    let (_, file, editor) = find_ready(&events).expect("EditReady");
    assert_eq!(std::fs::read(&file).unwrap(), b"old content");
    assert_eq!(file.file_name().unwrap(), "a.txt");
    assert!(file.starts_with(t.engine.paths.edit_dir()));
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(file.parent().unwrap()), 0o700);
    assert_eq!(editor.program, "vi");
    assert!(notices_of(&events).contains(&(Severity::Info, "Downloading a.txt to edit\u{2026}".to_string())));
}

#[tokio::test]
async fn the_temporary_copy_keeps_an_awkward_file_name() {
    let mut t = test_engine();
    let (session, fs) = t.add_session("prod");
    fs.file("/home/user/-my f\u{fc}le.txt", b"x", Some(1));

    let events = open_remote(&mut t, session, "/home/user/-my f\u{fc}le.txt").await;

    let (_, file, _) = find_ready(&events).expect("EditReady");
    assert_eq!(file.file_name().unwrap(), "-my f\u{fc}le.txt");
    assert!(file.is_absolute());
}

#[tokio::test]
async fn an_empty_remote_file_edits_normally() {
    let mut t = test_engine();
    let (session, fs) = t.add_session("prod");
    fs.file("/home/user/empty", b"", Some(1));

    let events = open_remote(&mut t, session, "/home/user/empty").await;

    let (_, file, _) = find_ready(&events).expect("EditReady");
    assert_eq!(std::fs::read(file).unwrap(), b"");
}

#[tokio::test]
async fn two_edits_of_the_same_file_use_separate_temporary_directories() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);

    let first = find_ready(&open_remote(&mut t, session, REMOTE).await).unwrap();
    let second = find_ready(&open_remote(&mut t, session, REMOTE).await).unwrap();

    assert_ne!(first.0, second.0);
    assert_ne!(first.1.parent(), second.1.parent());
}

#[tokio::test]
async fn a_remote_folder_is_refused_without_a_temporary_copy() {
    let mut t = test_engine();
    let (session, fs) = t.add_session("prod");
    fs.dir("/home/user/d");

    let events = open_remote(&mut t, session, "/home/user/d").await;

    assert!(find_ready(&events).is_none());
    assert!(error_notices(&events).iter().any(|message| message == "Can't edit a folder"));
    assert!(!t.engine.paths.edit_dir().exists());
}

#[tokio::test]
async fn a_remote_file_over_the_limit_is_refused_before_downloading() {
    let mut t = test_engine();
    let (session, fs) = t.add_session("prod");
    fs.file("/home/user/big", &vec![0u8; MAX_EDIT_BYTES as usize + 1], Some(1));

    let events = open_remote(&mut t, session, "/home/user/big").await;

    assert!(find_ready(&events).is_none());
    assert!(error_notices(&events).iter().any(|message| message == "big is too large to edit (limit 64 MiB)"));
    assert!(!t.engine.paths.edit_dir().exists());
}

#[tokio::test]
async fn a_missing_remote_file_is_an_error_and_leaves_nothing_behind() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);

    let events = open_remote(&mut t, session, "/home/user/nope.txt").await;

    assert!(find_ready(&events).is_none());
    assert!(error_notices(&events).iter().any(|message| message.starts_with("Edit failed: nope.txt")));
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn a_remote_file_that_cannot_be_read_leaves_no_temporary_copy() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    fs.fail_reads(REMOTE);

    let events = open_remote(&mut t, session, REMOTE).await;

    assert!(find_ready(&events).is_none());
    assert!(!error_notices(&events).is_empty());
    let leftovers = std::fs::read_dir(t.engine.paths.edit_dir()).map(|dir| dir.count()).unwrap_or(0);
    assert_eq!(leftovers, 0);
}

#[tokio::test]
async fn a_disconnected_session_cannot_be_edited() {
    let mut t = test_engine();

    t.engine.handle_command(Command::EditFile { location: Location::Session(77), path: PathBuf::from(REMOTE) });

    assert!(error_notices(&t.drain()).iter().any(|message| message == "Edit failed: session disconnected"));
}

#[tokio::test]
async fn cancelling_everything_during_the_download_removes_the_temporary_copy() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);

    t.engine.handle_command(Command::EditFile { location: Location::Session(session), path: PathBuf::from(REMOTE) });
    t.engine.handle_command(Command::CancelAllTransfers);
    t.run_internal().await;

    let events = t.drain();
    assert!(find_ready(&events).is_none());
    assert!(notices_of(&events).contains(&(Severity::Info, "Edit cancelled".to_string())));
    let leftovers = std::fs::read_dir(t.engine.paths.edit_dir()).map(|dir| dir.count()).unwrap_or(0);
    assert_eq!(leftovers, 0);
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn an_unchanged_remote_file_is_never_uploaded_and_its_copy_is_removed() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file, _) = find_ready(&open_remote(&mut t, session, REMOTE).await).unwrap();
    let dir = file.parent().unwrap().to_path_buf();

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Success });
    t.run_internal().await;

    let events = t.drain();
    assert!(notices_of(&events).contains(&(Severity::Info, "No changes to a.txt".to_string())));
    assert!(fs.written_sizes().is_empty());
    assert!(!dir.exists());
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn saving_identical_content_counts_as_unchanged() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file, _) = find_ready(&open_remote(&mut t, session, REMOTE).await).unwrap();
    std::fs::write(&file, b"old content").unwrap();

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Success });
    t.run_internal().await;

    assert!(notices_of(&t.drain()).contains(&(Severity::Info, "No changes to a.txt".to_string())));
    assert!(fs.written_sizes().is_empty());
}

#[tokio::test]
async fn an_editor_that_could_not_start_removes_the_copy() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);
    let (edit_id, file, _) = find_ready(&open_remote(&mut t, session, REMOTE).await).unwrap();
    let dir = file.parent().unwrap().to_path_buf();

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::LaunchFailed("gone".to_string()) });

    assert!(error_notices(&t.drain()).iter().any(|message| message.starts_with("Couldn't start the editor (gone)")));
    assert!(!dir.exists());
}

async fn edit_to(t: &mut TestEngine, session: u64, content: &[u8]) -> (u64, PathBuf) {
    let (edit_id, file, _) = find_ready(&open_remote(t, session, REMOTE).await).unwrap();
    std::fs::write(&file, content).unwrap();
    (edit_id, file)
}

async fn finish(t: &mut TestEngine, edit_id: u64) {
    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Success });
    t.run_internal().await;
}

fn question_of(events: &[Event]) -> Option<(u64, String, EditQuestionKind)> {
    events.iter().find_map(|event| match event {
        Event::EditQuestion { edit_id, name, kind } => Some((*edit_id, name.clone(), *kind)),
        _ => None,
    })
}

#[tokio::test]
async fn a_changed_file_is_uploaded_in_place_and_the_copy_removed() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new content!").await;
    let dir = file.parent().unwrap().to_path_buf();

    finish(&mut t, edit_id).await;
    t.run_internal().await;

    let events = t.drain();
    assert_eq!(fs.contents(REMOTE).unwrap(), b"new content!");
    assert_eq!(fs.written_sizes(), vec![(PathBuf::from(REMOTE), 12)]);
    assert!(notices_of(&events).contains(&(Severity::Info, "Uploaded a.txt".to_string())));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::LocationChanged { location: Location::Session(id) } if *id == session))
    );
    assert!(!dir.exists());
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn with_auto_upload_off_the_user_is_asked_and_yes_uploads() {
    let mut t = test_engine();
    t.engine.edit.settings.auto_upload = false;
    let (session, fs) = remote_session(&mut t);
    let (edit_id, _) = edit_to(&mut t, session, b"new").await;

    finish(&mut t, edit_id).await;

    let question = question_of(&t.drain()).expect("a question");
    assert_eq!(question, (edit_id, "a.txt".to_string(), EditQuestionKind::Upload));
    assert_eq!(fs.contents(REMOTE).unwrap(), b"old content");

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Upload });
    t.run_internal().await;

    assert_eq!(fs.contents(REMOTE).unwrap(), b"new");
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn with_auto_upload_off_no_keeps_the_copy_and_names_it() {
    let mut t = test_engine();
    t.engine.edit.settings.auto_upload = false;
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new").await;
    finish(&mut t, edit_id).await;
    t.drain();

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Cancel });

    let notices = notices_of(&t.drain());
    assert!(
        notices
            .iter()
            .any(|(_, message)| *message == format!("Edit cancelled; your changes are kept in {}", file.display())),
        "{notices:?}"
    );
    assert_eq!(fs.contents(REMOTE).unwrap(), b"old content");
    assert_eq!(std::fs::read(&file).unwrap(), b"new");
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn an_answer_that_does_not_fit_the_question_cancels_instead_of_guessing() {
    let mut t = test_engine();
    t.engine.edit.settings.auto_upload = false;
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new").await;
    finish(&mut t, edit_id).await;
    t.drain();

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::KeepCopy });

    assert_eq!(fs.contents(REMOTE).unwrap(), b"old content");
    assert!(file.exists());
}

#[tokio::test]
async fn an_answer_with_no_question_pending_is_ignored() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);
    let (edit_id, file, _) = find_ready(&open_remote(&mut t, session, REMOTE).await).unwrap();

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Upload });

    assert!(t.drain().is_empty());
    assert!(file.exists());
    assert!(t.engine.edit.sessions.contains_key(&edit_id));
}

async fn conflicted(t: &mut TestEngine, session: u64, fs: &FakeFs, remote_now: &[u8], remote_mtime: u64) -> u64 {
    let (edit_id, _) = edit_to(t, session, b"mine").await;
    fs.file(REMOTE, remote_now, Some(remote_mtime));
    finish(t, edit_id).await;
    t.run_internal().await;
    let question = question_of(&t.drain()).expect("a conflict question");
    assert_eq!(question, (edit_id, "a.txt".to_string(), EditQuestionKind::Conflict));
    edit_id
}

#[tokio::test]
async fn a_remote_file_that_changed_size_is_a_conflict_even_with_auto_upload() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);

    conflicted(&mut t, session, &fs, b"someone else was here", 10).await;

    assert_eq!(fs.contents(REMOTE).unwrap(), b"someone else was here");
    assert!(fs.written_sizes().is_empty());
}

#[tokio::test]
async fn a_remote_file_that_only_changed_its_modified_time_is_a_conflict() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);

    conflicted(&mut t, session, &fs, b"old content", 11).await;

    assert!(fs.written_sizes().is_empty());
}

#[tokio::test]
async fn a_remote_file_that_was_deleted_is_a_conflict() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, _) = edit_to(&mut t, session, b"mine").await;
    fs.delete(Path::new(REMOTE)).await.unwrap();

    finish(&mut t, edit_id).await;
    t.run_internal().await;

    assert_eq!(question_of(&t.drain()).map(|question| question.2), Some(EditQuestionKind::Conflict));
}

#[tokio::test]
async fn a_conflict_is_asked_again_even_after_the_upload_question() {
    let mut t = test_engine();
    t.engine.edit.settings.auto_upload = false;
    let (session, fs) = remote_session(&mut t);
    let (edit_id, _) = edit_to(&mut t, session, b"mine").await;
    finish(&mut t, edit_id).await;
    t.drain();
    fs.file(REMOTE, b"changed while the question was open", Some(99));

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Upload });
    t.run_internal().await;

    assert_eq!(question_of(&t.drain()).map(|question| question.2), Some(EditQuestionKind::Conflict));
    assert_eq!(fs.contents(REMOTE).unwrap(), b"changed while the question was open");
}

#[tokio::test]
async fn overwriting_a_conflict_replaces_the_remote_file() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let edit_id = conflicted(&mut t, session, &fs, b"someone else was here", 10).await;

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Upload });
    t.run_internal().await;

    assert_eq!(fs.contents(REMOTE).unwrap(), b"mine");
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn keeping_a_copy_leaves_the_remote_file_and_uploads_beside_it() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let edit_id = conflicted(&mut t, session, &fs, b"someone else was here", 10).await;

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::KeepCopy });
    t.run_internal().await;

    assert_eq!(fs.contents(REMOTE).unwrap(), b"someone else was here");
    let names: Vec<String> =
        fs.list(Path::new("/home/user")).await.unwrap().into_iter().map(|entry| entry.name).collect();
    let copy =
        names.iter().find(|name| name.starts_with("a.conflict-") && name.ends_with(".txt")).expect("a conflict copy");
    assert_eq!(fs.contents(format!("/home/user/{copy}")).unwrap(), b"mine");
    let notices = notices_of(&t.drain());
    assert!(notices.contains(&(Severity::Info, format!("Uploaded {copy}"))), "{notices:?}");
}

#[tokio::test]
async fn cancelling_a_conflict_keeps_everything_and_names_the_copy() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let edit_id = conflicted(&mut t, session, &fs, b"someone else was here", 10).await;
    let file = t.engine.edit.sessions[&edit_id].temp.as_ref().unwrap().file.clone();

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Cancel });

    assert!(notices_of(&t.drain()).iter().any(|(_, message)| message.contains(&file.display().to_string())));
    assert_eq!(fs.contents(REMOTE).unwrap(), b"someone else was here");
    assert_eq!(std::fs::read(&file).unwrap(), b"mine");
}

#[tokio::test]
async fn a_failed_upload_keeps_the_copy_and_says_where_it_is() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new").await;
    fs.fail_shutdown(REMOTE);

    finish(&mut t, edit_id).await;
    t.run_internal().await;

    let errors = error_notices(&t.drain());
    assert!(
        errors.iter().any(
            |message| message.contains("your changes are kept in") && message.contains(&file.display().to_string())
        ),
        "{errors:?}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"new");
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn a_session_lost_before_the_upload_keeps_the_copy() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new").await;
    t.engine.handle_command(Command::Disconnect { session });
    t.drain();

    finish(&mut t, edit_id).await;

    let errors = error_notices(&t.drain());
    assert!(
        errors.iter().any(|message| message.starts_with("Upload failed: session disconnected")
            && message.contains(&file.display().to_string())),
        "{errors:?}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"new");
}

#[tokio::test]
async fn a_conflict_copy_name_that_already_exists_fails_the_upload_and_overwrites_nothing() {
    let mut t = test_engine();
    t.engine.edit.clock = || chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 9, 30, 10, 15, 0).unwrap();
    let (session, fs) = remote_session(&mut t);
    let edit_id = conflicted(&mut t, session, &fs, b"someone else was here", 10).await;
    let file = t.engine.edit.sessions[&edit_id].temp.as_ref().unwrap().file.clone();
    fs.file("/home/user/a.conflict-20260930-101500.txt", b"an older copy", Some(5));

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::KeepCopy });
    t.run_internal().await;

    let errors = error_notices(&t.drain());
    assert!(
        errors
            .iter()
            .any(|message| message.contains("already exists") && message.contains(&file.display().to_string())),
        "{errors:?}"
    );
    assert_eq!(fs.contents("/home/user/a.conflict-20260930-101500.txt").unwrap(), b"an older copy");
    assert_eq!(fs.contents(REMOTE).unwrap(), b"someone else was here");
    assert_eq!(std::fs::read(&file).unwrap(), b"mine");
}

fn busy_events(events: &[Event]) -> Vec<bool> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::EditsBusy(busy) => Some(*busy),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_non_zero_exit_with_unchanged_content_removes_the_copy() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file, _) = find_ready(&open_remote(&mut t, session, REMOTE).await).unwrap();
    let dir = file.parent().unwrap().to_path_buf();

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Status(1) });
    t.run_internal().await;

    let events = t.drain();
    assert!(
        notices_of(&events)
            .contains(&(Severity::Warning, "Editor exited with status 1; nothing was uploaded".to_string()))
    );
    assert!(fs.written_sizes().is_empty());
    assert!(!dir.exists());
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn a_non_zero_exit_after_saving_keeps_the_copy_and_names_it_without_uploading() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"saved before the editor failed").await;

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Status(1) });
    t.run_internal().await;

    let notices = notices_of(&t.drain());
    assert!(
        notices.contains(&(
            Severity::Warning,
            format!("Editor exited with status 1; nothing was uploaded; your changes are kept in {}", file.display())
        )),
        "{notices:?}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"saved before the editor failed");
    assert_eq!(fs.contents(REMOTE).unwrap(), b"old content");
    assert!(fs.written_sizes().is_empty());
}

#[tokio::test]
async fn an_upload_failure_is_one_line_that_ends_with_where_the_copy_is() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new").await;
    fs.fail_shutdown(REMOTE);

    finish(&mut t, edit_id).await;
    t.run_internal().await;

    let errors = error_notices(&t.drain());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(!errors[0].contains('\n'), "{:?}", errors[0]);
    assert!(errors[0].starts_with("Upload failed: a.txt"), "{:?}", errors[0]);
    assert!(errors[0].ends_with(&format!("your changes are kept in {}", file.display())), "{:?}", errors[0]);
}

#[tokio::test]
async fn a_name_with_a_newline_never_splits_a_notice() {
    let mut t = test_engine();
    let (session, fs) = t.add_session("prod");
    fs.file("/home/user/x\ny.txt", b"same", Some(1));

    let events = open_remote(&mut t, session, "/home/user/x\ny.txt").await;
    let (edit_id, _, _) = find_ready(&events).unwrap();
    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Success });
    t.run_internal().await;

    let mut messages: Vec<String> = notices_of(&events).into_iter().map(|(_, message)| message).collect();
    messages.extend(notices_of(&t.drain()).into_iter().map(|(_, message)| message));
    assert!(messages.contains(&"Downloading x?y.txt to edit\u{2026}".to_string()), "{messages:?}");
    assert!(messages.contains(&"No changes to x?y.txt".to_string()), "{messages:?}");
    assert!(messages.iter().all(|message| !message.contains('\n')), "{messages:?}");
}

#[tokio::test]
async fn cancelling_is_a_warning_so_the_path_does_not_vanish_after_seconds() {
    let mut t = test_engine();
    t.engine.edit.settings.auto_upload = false;
    let (session, _fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new").await;
    finish(&mut t, edit_id).await;
    t.drain();

    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Cancel });

    let notices = notices_of(&t.drain());
    assert!(
        notices.contains(&(Severity::Warning, format!("Edit cancelled; your changes are kept in {}", file.display()))),
        "{notices:?}"
    );
}

#[tokio::test]
async fn a_second_finish_is_ignored_and_never_deletes_the_copy_being_saved() {
    let mut t = test_engine();
    let (session, fs) = remote_session(&mut t);
    let (edit_id, file) = edit_to(&mut t, session, b"new content!").await;

    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Success });
    t.engine.handle_command(Command::FinishEdit { edit_id, exit: EditorExit::Status(1) });
    t.run_internal().await;
    t.run_internal().await;

    assert_eq!(fs.contents(REMOTE).unwrap(), b"new content!");
    assert!(!file.exists());
}

#[tokio::test]
async fn finishing_before_the_editor_was_ever_started_is_ignored() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);
    t.engine.handle_command(Command::EditFile { location: Location::Session(session), path: PathBuf::from(REMOTE) });

    t.engine.handle_command(Command::FinishEdit { edit_id: 0, exit: EditorExit::Success });

    assert!(error_notices(&t.drain()).is_empty());
    assert!(t.engine.edit.sessions.contains_key(&0));
}

#[tokio::test]
async fn an_existing_temporary_folder_is_never_reused() {
    let mut t = test_engine();
    t.engine.edit.clock = || chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 9, 30, 10, 15, 0).unwrap();
    let (session, _fs) = remote_session(&mut t);
    let taken = t.engine.paths.edit_dir().join(format!("{}-0", (t.engine.edit.clock)().timestamp_millis()));
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("precious"), b"someone else's copy").unwrap();

    let events = open_remote(&mut t, session, REMOTE).await;

    assert!(find_ready(&events).is_none());
    assert!(!error_notices(&events).is_empty());
    assert_eq!(std::fs::read(taken.join("precious")).unwrap(), b"someone else's copy");
}

#[tokio::test]
async fn a_download_that_finished_just_before_cancelling_does_not_open_the_editor() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);
    let (edit_id, cancel) = t.engine.new_edit_session(
        Location::Session(session),
        PathBuf::from(REMOTE),
        EditorCommand { program: "vi".to_string(), args: Vec::new() },
    );
    let dir = t.engine.paths.edit_dir().join("finished-download");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, b"old content").unwrap();
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);

    t.engine.handle_edit_event(EditEvent::Prepared {
        edit_id,
        result: Prepared::Ready {
            temp: TempCopy { dir: dir.clone(), file },
            baseline: Baseline { size: 11, modified: Some(10), hash: 0 },
        },
    });

    let events = t.drain();
    assert!(find_ready(&events).is_none());
    assert!(notices_of(&events).contains(&(Severity::Info, "Edit cancelled".to_string())));
    assert!(!dir.exists());
    assert!(t.engine.edit.sessions.is_empty());
}

#[tokio::test]
async fn the_ui_is_told_when_an_edit_is_being_saved_and_when_it_is_done() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);
    let (edit_id, _) = edit_to(&mut t, session, b"new content!").await;

    finish(&mut t, edit_id).await;
    t.run_internal().await;

    assert_eq!(busy_events(&t.drain()), [true, false]);
}

#[tokio::test]
async fn waiting_for_an_answer_is_not_busy() {
    let mut t = test_engine();
    t.engine.edit.settings.auto_upload = false;
    let (session, _fs) = remote_session(&mut t);
    let (edit_id, _) = edit_to(&mut t, session, b"new").await;

    finish(&mut t, edit_id).await;

    assert_eq!(busy_events(&t.drain()), [true, false]);
    t.engine.handle_command(Command::ResolveEdit { edit_id, choice: EditChoice::Upload });
    let after_answer = busy_events(&t.drain());
    t.run_internal().await;
    assert_eq!([after_answer, busy_events(&t.drain())].concat(), [true, false]);
}

#[tokio::test]
async fn an_edit_that_is_only_being_edited_is_not_busy() {
    let mut t = test_engine();
    let (session, _fs) = remote_session(&mut t);

    let events = open_remote(&mut t, session, REMOTE).await;

    assert!(busy_events(&events).is_empty());
}
