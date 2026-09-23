use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use super::*;
use crate::transfer::plan::{DirectoryPlan, ExistingFile, PlannedFile};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

fn file(name: &str, conflict: bool) -> PlannedFile {
    PlannedFile { local_path: PathBuf::from(format!("/local/{name}")), remote_path: format!("/remote/{name}"), display_name: name.to_string(), size: 1, existing: conflict.then_some(ExistingFile { size: 2, modified: None, is_dir: false }), source_modified: None }
}

fn plan(files: Vec<PlannedFile>) -> DirectoryPlan {
    let names: HashSet<String> = files.iter().map(|file| file.display_name.clone()).collect();
    DirectoryPlan { files, skipped_symlinks: 0, taken_names: HashMap::from([(PathBuf::from("/remote"), names)]) }
}

fn review(app: &mut App, files: Vec<PlannedFile>) -> u64 {
    let batch_id = app.transfers.start_batch("copy".to_string());
    app.review_or_apply_plan(batch_id, 1, Direction::Upload, plan(files));
    batch_id
}

fn press(app: &mut App, code: KeyCode) {
    app.apply_dialog_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn queued_remote_paths(app: &App) -> Vec<String> {
    app.transfers.jobs().map(|job| job.remote_path.clone()).collect()
}

fn connection_entry() -> ConnectionEntry {
    ConnectionEntry { name: "test".to_string(), host: "h".to_string(), port: 22, username: "u".to_string(), identity_file: None, remote_path: None, password: None, source: ConnectionSource::Profile }
}

#[test]
fn a_plan_without_conflicts_is_queued_without_a_prompt() {
    let (_dir, mut app) = app();

    review(&mut app, vec![file("a.txt", false)]);

    assert!(app.dialog.is_none());
    assert_eq!(queued_remote_paths(&app), vec!["/remote/a.txt".to_string()]);
}

#[test]
fn conflicts_open_a_prompt_and_answers_decide_what_is_queued() {
    let (_dir, mut app) = app();
    review(&mut app, vec![file("a.txt", false), file("b.txt", true), file("c.txt", true)]);

    assert!(matches!(&app.dialog, Some(Dialog::Conflict(dialog)) if dialog.total == 2 && dialog.index == 0));
    press(&mut app, KeyCode::Char('s'));
    assert!(matches!(&app.dialog, Some(Dialog::Conflict(dialog)) if dialog.index == 1));
    press(&mut app, KeyCode::Char('r'));

    assert!(app.dialog.is_none());
    assert_eq!(queued_remote_paths(&app), vec!["/remote/a.txt".to_string(), "/remote/c (1).txt".to_string()]);
}

#[test]
fn same_answer_for_the_rest_finishes_the_review() {
    let (_dir, mut app) = app();
    review(&mut app, vec![file("a.txt", true), file("b.txt", true), file("c.txt", true)]);

    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('o'));

    assert!(app.dialog.is_none());
    assert_eq!(app.transfers.jobs().count(), 3);
}

#[test]
fn cancelling_the_copy_queues_nothing_and_says_so() {
    let (_dir, mut app) = app();
    let batch_id = review(&mut app, vec![file("a.txt", false), file("b.txt", true)]);

    press(&mut app, KeyCode::Esc);

    assert!(app.dialog.is_none());
    assert_eq!(app.transfers.jobs().count(), 0);
    assert_eq!(app.notifications.current().unwrap().message, "Copy cancelled");
    assert_eq!(app.transfers.batch_label(batch_id), None);
}

#[test]
fn the_prompt_waits_for_an_open_dialog_to_close() {
    let (_dir, mut app) = app();
    app.dialog = Some(Dialog::Confirm(ConfirmDialog::new("Delete \"x\"?")));
    app.pending_action = Some(PendingAction::Delete);

    review(&mut app, vec![file("a.txt", true)]);
    assert!(matches!(app.dialog, Some(Dialog::Confirm(_))));

    press(&mut app, KeyCode::Char('n'));

    assert!(matches!(app.dialog, Some(Dialog::Conflict(_))));
}

#[test]
fn a_second_copy_waits_its_turn() {
    let (_dir, mut app) = app();
    review(&mut app, vec![file("a.txt", true)]);
    review(&mut app, vec![file("b.txt", true)]);

    press(&mut app, KeyCode::Char('o'));

    assert!(matches!(&app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "b.txt"));
    assert_eq!(queued_remote_paths(&app), vec!["/remote/a.txt".to_string()]);
}

#[test]
fn a_policy_other_than_ask_never_prompts() {
    let (_dir, mut app) = app();
    app.on_conflict = ConflictPolicy::Skip;

    review(&mut app, vec![file("a.txt", false), file("b.txt", true)]);

    assert!(app.dialog.is_none());
    assert_eq!(queued_remote_paths(&app), vec!["/remote/a.txt".to_string()]);
}

#[test]
fn disconnecting_drops_that_sessions_waiting_copies_and_counts_them() {
    let (_dir, mut app) = app();
    let entry = connection_entry();
    let session_id = app.sessions.insert(entry.clone(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));
    app.connections = vec![entry];
    app.screen = Screen::Connections;
    let batch_id = app.transfers.start_batch("copy".to_string());
    app.conflict_reviews.push_back(ConflictReview { batch_id, session_id, direction: Direction::Upload, plan: plan(vec![file("a.txt", true)]), conflicts: vec![0], answers: Vec::new() });

    app.disconnect_selected();

    assert!(app.conflict_reviews.is_empty());
    assert!(app.notifications.current().unwrap().message.contains("1 transfer cancelled"));
}

#[test]
fn a_waiting_copy_shows_on_the_transfers_screen() {
    let (_dir, mut app) = app();
    app.dialog = Some(Dialog::Confirm(ConfirmDialog::new("busy")));
    review(&mut app, vec![file("a.txt", true)]);

    let rows = app.transfer_rows();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, crate::transfer::rows::RowState::AwaitingAnswer);
    assert_eq!(rows[0].label, "copy");
}

#[test]
fn copying_only_files_now_goes_through_the_scan() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
    let mut app = App::at(dir.path().to_path_buf()).unwrap();
    app.local.cursor = app.local.rows().len() - 1;
    app.sessions.insert(connection_entry(), PanelState::from_listing(PathBuf::from("/remote"), Vec::new()));

    app.start_copy();

    assert_eq!(app.notifications.current().unwrap().message, "Copy failed: session disconnected");
    assert_eq!(app.transfers.jobs().count(), 0);
}

fn status_line(app: &App) -> String {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render_status(frame, frame.area())).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn a_prompt_waits_while_searching_and_opens_when_search_closes() {
    let (_dir, mut app) = app();
    app.apply_action(Action::OpenSearch);
    assert_eq!(app.screen, Screen::Search);

    review(&mut app, vec![file("photo.jpg", true)]);

    assert!(app.dialog.is_none());
    assert!(status_line(&app).contains("waiting for your answer"));

    app.apply_search_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(app.screen, Screen::Files);
    assert!(matches!(app.dialog, Some(Dialog::Conflict(_))));
}

#[test]
fn the_prompt_shows_the_files_path_within_the_copy() {
    let (_dir, mut app) = app();

    review(&mut app, vec![file("sub/index.html", true)]);

    assert!(matches!(&app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "sub/index.html"));
}

#[test]
fn a_dialog_that_replaces_the_prompt_hands_back_to_the_same_conflict() {
    let (_dir, mut app) = app();
    review(&mut app, vec![file("a.txt", true), file("b.txt", true)]);
    press(&mut app, KeyCode::Char('s'));
    app.dialog = Some(Dialog::TextInput(TextInputDialog::new("Password", "")));
    app.pending_action = Some(PendingAction::SubmitPassword);

    press(&mut app, KeyCode::Esc);

    assert!(matches!(&app.dialog, Some(Dialog::Conflict(dialog)) if dialog.index == 1 && dialog.file_name == "b.txt"));
}

#[test]
fn skipped_existing_files_are_reported() {
    let (_dir, mut app) = app();
    app.on_conflict = ConflictPolicy::Skip;

    review(&mut app, vec![file("a.txt", false), file("b.txt", true), file("c.txt", true)]);

    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Info);
    assert_eq!(notification.message, "Skipped 2 existing files");
}

#[test]
fn files_blocked_by_a_folder_are_reported() {
    let (_dir, mut app) = app();
    app.on_conflict = ConflictPolicy::Overwrite;
    let mut blocked = file("photos", true);
    blocked.existing = Some(ExistingFile { size: 0, modified: None, is_dir: true });

    review(&mut app, vec![blocked]);

    let notification = app.notifications.current().unwrap();
    assert_eq!(notification.severity, Severity::Warning);
    assert_eq!(notification.message, "Skipped 1 file because a folder with the same name exists");
}
