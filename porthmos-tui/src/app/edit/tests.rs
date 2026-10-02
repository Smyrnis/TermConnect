use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use porthmos_core::edit::{EditChoice, EditQuestionKind, EditorCommand, EditorExit};

use super::*;
use crate::app::testing::{TestApp, entry, test_app};

fn app_on_a_local_file() -> TestApp {
    let mut test = test_app(Path::new("/d"));
    test.list_local(vec![entry(Path::new("/d"), "notes.txt", false), entry(Path::new("/d"), "sub", true)]);
    test
}

fn move_to(test: &mut TestApp, name: &str) {
    let position = test
        .app
        .local
        .rows()
        .iter()
        .position(|row| matches!(row, porthmos_core::listing::Row::Entry(entry) if entry.name == name))
        .unwrap();
    test.app.local.cursor = position;
}

#[test]
fn f3_on_a_local_file_asks_the_core_to_edit_it() {
    let mut test = app_on_a_local_file();
    move_to(&mut test, "notes.txt");

    test.app.apply_action(Action::Edit);

    assert_eq!(
        test.sent(),
        vec![Command::EditFile { location: Location::Local, path: Path::new("/d/notes.txt").to_path_buf() }]
    );
}

#[test]
fn f3_on_a_folder_warns_and_sends_nothing() {
    let mut test = app_on_a_local_file();
    move_to(&mut test, "sub");

    test.app.apply_action(Action::Edit);

    assert!(test.sent().is_empty());
    assert_eq!(test.notification().as_deref(), Some("Can't edit a folder"));
}

#[test]
fn f3_on_the_parent_row_warns_and_sends_nothing() {
    let mut test = app_on_a_local_file();
    test.app.local.cursor = 0;
    assert!(test.app.local.on_parent_row());

    test.app.apply_action(Action::Edit);

    assert!(test.sent().is_empty());
    assert_eq!(test.notification().as_deref(), Some("Can't edit a folder"));
}

#[test]
fn f3_in_an_empty_folder_does_nothing() {
    let mut test = test_app(Path::new("/"));
    test.list_local(Vec::new());

    test.app.apply_action(Action::Edit);

    assert!(test.sent().is_empty());
}

#[test]
fn f3_on_the_remote_panel_without_a_session_warns() {
    let mut test = app_on_a_local_file();
    test.app.active_panel = ActivePanel::Remote;

    test.app.apply_action(Action::Edit);

    assert!(test.sent().is_empty());
    assert_eq!(test.notification().as_deref(), Some("Connect to a remote server first"));
}

#[test]
fn f3_on_a_remote_file_names_the_session_and_path() {
    let mut test = test_app(Path::new("/d"));
    let session = test.connect(3, "prod");
    test.list_remote(session, "/srv", vec![entry(Path::new("/srv"), "app.conf", false)]);
    test.sent();
    test.app.active_panel = ActivePanel::Remote;
    let remote = test.app.sessions.active_mut().unwrap();
    let position =
        remote.panel.rows().iter().position(|row| matches!(row, porthmos_core::listing::Row::Entry(_))).unwrap();
    remote.panel.cursor = position;

    test.app.apply_action(Action::Edit);

    assert_eq!(
        test.sent(),
        vec![Command::EditFile { location: Location::Session(3), path: Path::new("/srv/app.conf").to_path_buf() }]
    );
}

#[test]
fn f3_on_another_screen_does_nothing() {
    let mut test = app_on_a_local_file();
    move_to(&mut test, "notes.txt");
    test.app.apply_action(Action::OpenTransfers);

    test.app.apply_action(Action::Edit);

    assert!(test.sent().is_empty());
}

#[test]
fn the_f3_key_reaches_the_action() {
    let mut test = app_on_a_local_file();
    move_to(&mut test, "notes.txt");

    test.app.apply_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::F(3),
        crossterm::event::KeyModifiers::NONE,
    ));

    assert_eq!(test.sent().len(), 1);
}

#[test]
fn the_file_is_the_last_argument_and_nothing_goes_through_a_shell() {
    let editor = EditorCommand { program: "code".to_string(), args: vec!["--wait".to_string(), "-n".to_string()] };

    let command = editor_process(&editor, Path::new("/tmp/-weird name; rm -rf.txt"));

    assert_eq!(command.get_program(), "code");
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, ["--wait", "-n", "/tmp/-weird name; rm -rf.txt"]);
}

#[test]
fn the_exit_status_maps_to_the_editor_outcome() {
    let ok = std::process::Command::new("sh").args(["-c", "exit 0"]).status();
    let failed = std::process::Command::new("sh").args(["-c", "exit 3"]).status();
    let missing = std::process::Command::new("/nonexistent/editor").status();

    assert_eq!(editor_exit(ok), EditorExit::Success);
    assert_eq!(editor_exit(failed), EditorExit::Status(3));
    assert!(matches!(editor_exit(missing), EditorExit::LaunchFailed(message) if !message.is_empty()));
}

#[test]
fn a_process_killed_by_a_signal_is_a_failed_status() {
    let killed = std::process::Command::new("sh").args(["-c", "kill -9 $$"]).status();

    assert_eq!(editor_exit(killed), EditorExit::Status(-1));
}

#[test]
fn an_edit_ready_event_outside_the_run_loop_changes_nothing() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_core_event(Event::EditReady {
        edit_id: 1,
        file: Path::new("/tmp/x").to_path_buf(),
        editor: EditorCommand { program: "vi".to_string(), args: Vec::new() },
    });

    assert!(test.sent().is_empty());
    assert!(test.app.dialog.is_none());
}

fn press(test: &mut TestApp, code: KeyCode) {
    test.app.apply_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ask(test: &mut TestApp, name: &str, kind: EditQuestionKind) {
    test.app.apply_core_event(Event::EditQuestion { edit_id: 4, name: name.to_string(), kind });
}

#[test]
fn the_upload_question_is_a_confirm_dialog_and_yes_uploads() {
    let mut test = test_app(Path::new("/d"));

    ask(&mut test, "app.conf", EditQuestionKind::Upload);

    match &test.app.dialog {
        Some(Dialog::Confirm(dialog)) => assert!(dialog.message.contains("Upload your changes to app.conf?")),
        _ => panic!("expected a confirm dialog"),
    }
    press(&mut test, KeyCode::Char('y'));
    assert_eq!(test.sent(), vec![Command::ResolveEdit { edit_id: 4, choice: EditChoice::Upload }]);
    assert!(test.app.dialog.is_none());
}

#[test]
fn no_and_escape_cancel_the_upload_question() {
    let mut test = test_app(Path::new("/d"));

    ask(&mut test, "a", EditQuestionKind::Upload);
    press(&mut test, KeyCode::Char('n'));
    assert_eq!(test.sent(), vec![Command::ResolveEdit { edit_id: 4, choice: EditChoice::Cancel }]);

    ask(&mut test, "a", EditQuestionKind::Upload);
    press(&mut test, KeyCode::Esc);
    assert_eq!(test.sent(), vec![Command::ResolveEdit { edit_id: 4, choice: EditChoice::Cancel }]);
}

#[test]
fn the_conflict_question_lists_three_choices() {
    let mut test = test_app(Path::new("/d"));

    ask(&mut test, "app.conf", EditQuestionKind::Conflict);

    match &test.app.dialog {
        Some(Dialog::List(dialog)) => {
            assert!(dialog.title.contains("app.conf changed on the server while you were editing"));
            assert_eq!(dialog.items, ["Overwrite", "Keep mine as a copy", "Cancel"]);
        }
        _ => panic!("expected a list dialog"),
    }
}

#[test]
fn the_conflict_list_starts_on_the_safe_choice_and_every_choice_sends_the_matching_answer() {
    let expected = [
        (vec![KeyCode::Up], EditChoice::Upload),
        (vec![], EditChoice::KeepCopy),
        (vec![KeyCode::Down], EditChoice::Cancel),
    ];
    for (moves, choice) in expected {
        let mut test = test_app(Path::new("/d"));
        ask(&mut test, "a", EditQuestionKind::Conflict);
        for code in moves {
            press(&mut test, code);
        }

        press(&mut test, KeyCode::Enter);

        assert_eq!(test.sent(), vec![Command::ResolveEdit { edit_id: 4, choice }]);
        assert!(test.app.dialog.is_none());
    }
}

#[test]
fn escape_cancels_the_conflict_question() {
    let mut test = test_app(Path::new("/d"));
    ask(&mut test, "a", EditQuestionKind::Conflict);

    press(&mut test, KeyCode::Esc);

    assert_eq!(test.sent(), vec![Command::ResolveEdit { edit_id: 4, choice: EditChoice::Cancel }]);
}

#[test]
fn control_characters_in_the_name_never_reach_the_dialogs() {
    let mut test = test_app(Path::new("/d"));

    ask(&mut test, "evil\u{1b}[31m\nname", EditQuestionKind::Upload);
    match &test.app.dialog {
        Some(Dialog::Confirm(dialog)) => {
            assert!(!dialog.message.chars().any(char::is_control), "{:?}", dialog.message)
        }
        _ => panic!("expected a confirm dialog"),
    }

    press(&mut test, KeyCode::Char('n'));
    ask(&mut test, "evil\u{1b}[31m\nname", EditQuestionKind::Conflict);
    match &test.app.dialog {
        Some(Dialog::List(dialog)) => assert!(!dialog.title.chars().any(char::is_control), "{:?}", dialog.title),
        _ => panic!("expected a list dialog"),
    }
}

fn ask_for(test: &mut TestApp, edit_id: u64, name: &str, kind: EditQuestionKind) {
    test.app.apply_core_event(Event::EditQuestion { edit_id, name: name.to_string(), kind });
}

#[test]
fn two_questions_are_asked_one_after_the_other_and_none_is_lost() {
    let mut test = test_app(Path::new("/d"));

    ask_for(&mut test, 1, "first", EditQuestionKind::Upload);
    ask_for(&mut test, 2, "second", EditQuestionKind::Upload);
    match &test.app.dialog {
        Some(Dialog::Confirm(dialog)) => assert!(dialog.message.contains("first"), "{}", dialog.message),
        _ => panic!("expected the first question"),
    }
    press(&mut test, KeyCode::Char('y'));
    match &test.app.dialog {
        Some(Dialog::Confirm(dialog)) => assert!(dialog.message.contains("second"), "{}", dialog.message),
        _ => panic!("expected the second question"),
    }
    press(&mut test, KeyCode::Char('y'));

    assert_eq!(
        test.sent(),
        vec![
            Command::ResolveEdit { edit_id: 1, choice: EditChoice::Upload },
            Command::ResolveEdit { edit_id: 2, choice: EditChoice::Upload },
        ]
    );
    assert!(test.app.dialog.is_none());
}

#[test]
fn a_question_waits_for_a_dialog_that_is_already_open() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::Mkdir);
    assert!(matches!(test.app.dialog, Some(Dialog::TextInput(_))));

    ask_for(&mut test, 1, "a", EditQuestionKind::Upload);

    assert!(matches!(test.app.dialog, Some(Dialog::TextInput(_))));
    press(&mut test, KeyCode::Esc);
    assert!(matches!(test.app.dialog, Some(Dialog::Confirm(_))));
    press(&mut test, KeyCode::Char('y'));
    assert_eq!(test.sent(), vec![Command::ResolveEdit { edit_id: 1, choice: EditChoice::Upload }]);
}

#[test]
fn a_password_prompt_does_not_strand_an_open_edit_question() {
    let mut test = test_app(Path::new("/d"));
    ask_for(&mut test, 1, "a", EditQuestionKind::Upload);

    test.app.apply_core_event(Event::Question {
        request_id: 9,
        question: porthmos_core::Question::Password { username: "me".to_string(), name: "prod".to_string() },
    });
    assert!(!matches!(test.app.dialog, Some(Dialog::Confirm(_))));
    press(&mut test, KeyCode::Esc);
    assert!(matches!(test.app.dialog, Some(Dialog::Confirm(_))));
    press(&mut test, KeyCode::Char('y'));

    let sent = test.sent();
    assert!(sent.contains(&Command::Answer { request_id: 9, answer: None, save: false }), "{sent:?}");
    assert!(sent.contains(&Command::ResolveEdit { edit_id: 1, choice: EditChoice::Upload }), "{sent:?}");
}

#[test]
fn quitting_while_an_edit_is_being_saved_asks_first() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_core_event(Event::EditsBusy(true));

    test.app.apply_action(Action::Quit);

    assert!(!test.app.should_quit);
    match &test.app.dialog {
        Some(Dialog::Confirm(dialog)) => assert!(dialog.message.contains("still being saved"), "{}", dialog.message),
        _ => panic!("expected a confirm dialog"),
    }
    press(&mut test, KeyCode::Char('n'));
    assert!(!test.app.should_quit);
    assert!(test.app.dialog.is_none());

    test.app.apply_action(Action::Quit);
    press(&mut test, KeyCode::Char('y'));
    assert!(test.app.should_quit);
}

#[test]
fn quitting_when_no_edit_is_being_saved_quits_at_once() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_core_event(Event::EditsBusy(true));
    test.app.apply_core_event(Event::EditsBusy(false));

    test.app.apply_action(Action::Quit);

    assert!(test.app.should_quit);
    assert!(test.app.dialog.is_none());
}
