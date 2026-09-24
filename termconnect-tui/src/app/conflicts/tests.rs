use std::path::Path;

use termconnect_core::transfer::plan::ExistingFile;

use super::*;
use crate::app::testing::{TestApp, test_app};

fn app() -> TestApp {
    test_app(Path::new("/d"))
}

fn file(name: &str, conflict: bool) -> ConflictInfo {
    ConflictInfo {
        display_name: name.to_string(),
        existing: conflict.then_some(ExistingFile { size: 2, modified: None, is_dir: false }),
        partial: None,
        new_size: 1,
        new_modified: None,
    }
}

fn partial_only(name: &str) -> ConflictInfo {
    let mut partial = file(name, false);
    partial.partial = Some(ExistingFile { size: 0, modified: None, is_dir: false });
    partial
}

fn review(test: &mut TestApp, batch_id: u64, files: Vec<ConflictInfo>) {
    test.app.apply_core_event(Event::ConflictsFound { batch_id, files });
}

fn press(test: &mut TestApp, code: KeyCode) {
    test.app.apply_dialog_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn resolutions(test: &mut TestApp) -> Vec<(u64, Option<Vec<Resolution>>)> {
    test.sent()
        .into_iter()
        .filter_map(|command| match command {
            Command::ResolveConflicts { batch_id, answers } => Some((batch_id, answers)),
            _ => None,
        })
        .collect()
}

#[test]
fn conflicts_open_a_prompt_and_answers_decide_what_is_queued() {
    let mut test = app();
    review(&mut test, 1, vec![file("b.txt", true), file("c.txt", true)]);

    assert!(matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.total == 2 && dialog.index == 0));
    press(&mut test, KeyCode::Char('s'));
    assert!(matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.index == 1));
    press(&mut test, KeyCode::Char('r'));

    assert!(test.app.dialog.is_none());
    assert_eq!(resolutions(&mut test), vec![(1, Some(vec![Resolution::Skip, Resolution::Rename]))]);
}

#[test]
fn same_answer_for_the_rest_finishes_the_review() {
    let mut test = app();
    review(&mut test, 1, vec![file("a.txt", true), file("b.txt", true), file("c.txt", true)]);

    press(&mut test, KeyCode::Char('a'));
    press(&mut test, KeyCode::Char('o'));

    assert!(test.app.dialog.is_none());
    assert_eq!(resolutions(&mut test), vec![(1, Some(vec![Resolution::Overwrite; 3]))]);
}

#[test]
fn cancelling_the_copy_queues_nothing_and_says_so() {
    let mut test = app();
    review(&mut test, 1, vec![file("b.txt", true)]);

    press(&mut test, KeyCode::Esc);

    assert!(test.app.dialog.is_none());
    assert_eq!(resolutions(&mut test), vec![(1, None)]);
}

#[test]
fn the_prompt_waits_for_an_open_dialog_to_close() {
    let mut test = app();
    test.app.dialog = Some(Dialog::Confirm(ConfirmDialog::new("Delete \"x\"?")));
    test.app.pending_action = Some(PendingAction::Delete);

    review(&mut test, 1, vec![file("a.txt", true)]);
    assert!(matches!(test.app.dialog, Some(Dialog::Confirm(_))));

    press(&mut test, KeyCode::Char('n'));

    assert!(matches!(test.app.dialog, Some(Dialog::Conflict(_))));
}

#[test]
fn a_second_copy_waits_its_turn() {
    let mut test = app();
    review(&mut test, 1, vec![file("a.txt", true)]);
    review(&mut test, 2, vec![file("b.txt", true)]);

    press(&mut test, KeyCode::Char('o'));

    assert!(matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "b.txt"));
    assert_eq!(resolutions(&mut test), vec![(1, Some(vec![Resolution::Overwrite]))]);
}

#[test]
fn disconnecting_drops_that_sessions_waiting_copies_and_counts_them() {
    let mut test = app();
    review(&mut test, 1, vec![file("a.txt", true)]);
    review(&mut test, 2, vec![file("b.txt", true)]);

    test.app.apply_core_event(Event::ConflictsWithdrawn { batch_ids: vec![1] });

    assert_eq!(test.app.conflict_prompts.len(), 1);
    assert!(matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "b.txt"));
}

#[test]
fn withdrawing_a_copy_that_is_not_showing_keeps_the_current_prompt() {
    let mut test = app();
    review(&mut test, 1, vec![file("a.txt", true)]);
    review(&mut test, 2, vec![file("b.txt", true)]);

    test.app.apply_core_event(Event::ConflictsWithdrawn { batch_ids: vec![2] });

    assert!(matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "a.txt"));
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
    let mut test = app();
    test.app.apply_action(Action::OpenSearch);
    assert_eq!(test.app.screen, Screen::Search);

    review(&mut test, 1, vec![file("photo.jpg", true)]);

    assert!(test.app.dialog.is_none());
    assert!(status_line(&test.app).contains("waiting for your answer"));

    test.app.apply_search_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(test.app.screen, Screen::Files);
    assert!(matches!(test.app.dialog, Some(Dialog::Conflict(_))));
}

#[test]
fn the_prompt_shows_the_files_path_within_the_copy() {
    let mut test = app();

    review(&mut test, 1, vec![file("sub/index.html", true)]);

    assert!(matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "sub/index.html"));
}

#[test]
fn a_dialog_that_replaces_the_prompt_hands_back_to_the_same_conflict() {
    let mut test = app();
    review(&mut test, 1, vec![file("a.txt", true), file("b.txt", true)]);
    press(&mut test, KeyCode::Char('s'));
    test.app.dialog = Some(Dialog::TextInput(TextInputDialog::new("Password", "")));
    test.app.pending_action = Some(PendingAction::SubmitPassword { request_id: 0 });

    press(&mut test, KeyCode::Esc);

    assert!(
        matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.index == 1 && dialog.file_name == "b.txt")
    );
}

#[test]
fn a_partial_found_at_copy_time_opens_the_prompt_with_resume() {
    let mut test = app();

    review(&mut test, 1, vec![partial_only("big.iso")]);
    assert!(
        matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.partial.is_some() && dialog.existing.is_none())
    );
    press(&mut test, KeyCode::Char('u'));

    assert_eq!(resolutions(&mut test), vec![(1, Some(vec![Resolution::Resume]))]);
}

#[test]
fn resume_for_the_rest_leaves_complete_files_to_their_own_prompt() {
    let mut test = app();
    review(&mut test, 1, vec![partial_only("a.iso"), file("b.txt", true)]);

    press(&mut test, KeyCode::Char('a'));
    press(&mut test, KeyCode::Char('u'));

    assert!(
        matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "b.txt" && dialog.index == 1)
    );
    press(&mut test, KeyCode::Char('s'));
    assert_eq!(resolutions(&mut test), vec![(1, Some(vec![Resolution::Resume, Resolution::Skip]))]);
}

#[test]
fn start_over_for_the_rest_never_overwrites_a_complete_file() {
    let mut test = app();
    review(&mut test, 1, vec![partial_only("a.iso"), file("b.txt", true)]);

    press(&mut test, KeyCode::Char('a'));
    press(&mut test, KeyCode::Char('o'));

    assert!(matches!(&test.app.dialog, Some(Dialog::Conflict(dialog)) if dialog.file_name == "b.txt"));
}
