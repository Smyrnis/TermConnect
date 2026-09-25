use std::path::Path;

use super::*;
use crate::app::testing::{TestApp, entry, test_app};

fn app_in(dir: &str) -> TestApp {
    test_app(Path::new(dir))
}

fn connection(name: &str, source: ConnectionSource) -> ConnectionEntry {
    ConnectionEntry {
        name: name.to_string(),
        protocol: "sftp".to_string(),
        host: format!("{name}.example.com"),
        port: 22,
        username: "user".to_string(),
        password: None,
        options: Default::default(),
        source,
    }
}

#[test]
fn quit_action_sets_should_quit() {
    let mut test = app_in("/d");
    test.app.apply_action(Action::Quit);
    assert!(test.app.should_quit);
}

#[test]
fn help_action_shows_the_overlay() {
    let mut test = app_in("/d");
    test.app.apply_action(Action::Help);
    assert!(test.app.help_visible);
}

#[test]
fn switch_panel_action_toggles_active_panel() {
    let mut test = app_in("/d");
    assert_eq!(test.app.active_panel, ActivePanel::Local);
    test.app.apply_action(Action::SwitchPanel);
    assert_eq!(test.app.active_panel, ActivePanel::Remote);
}

#[test]
fn noop_action_does_not_change_state() {
    let mut test = app_in("/d");
    test.app.apply_action(Action::Noop);
    assert!(!test.app.should_quit);
    assert_eq!(test.app.active_panel, ActivePanel::Local);
    assert!(test.sent().is_empty());
}

#[test]
fn panel_actions_are_ignored_when_remote_panel_has_no_listing_yet() {
    let mut test = app_in("/d");
    test.list_local(vec![entry(Path::new("/d"), "child", true)]);
    test.app.apply_action(Action::SwitchPanel);

    let cursor_before = test.app.local.cursor;
    test.app.apply_action(Action::Down);

    assert_eq!(test.app.local.cursor, cursor_before);
    assert!(test.sent().is_empty());
}

#[test]
fn remote_panel_navigation_works_once_a_listing_exists() {
    let mut test = app_in("/d");
    let session = test.connect(1, "test");
    test.list_remote(session, "/home/user", vec![entry(Path::new("/home/user"), "child", true)]);
    test.app.active_panel = ActivePanel::Remote;

    test.app.apply_action(Action::Down);

    assert_eq!(test.app.sessions.active().unwrap().panel.cursor, 1);
}

#[test]
fn opening_a_local_folder_asks_for_its_listing() {
    let mut test = app_in("/d");
    test.list_local(vec![entry(Path::new("/d"), "child", true)]);
    test.app.local.cursor = 1;

    test.app.apply_action(Action::Open);

    assert_eq!(test.sent(), vec![Command::List { location: Location::Local, path: Some(PathBuf::from("/d/child")) }]);
}

#[test]
fn opening_a_remote_folder_asks_that_session_for_its_listing() {
    let mut test = app_in("/d");
    let session = test.connect(4, "test");
    test.list_remote(session, "/home/user", vec![entry(Path::new("/home/user"), "child", true)]);
    test.app.active_panel = ActivePanel::Remote;
    test.app.sessions.active_mut().unwrap().panel.cursor = 1;

    test.app.apply_action(Action::Open);

    assert_eq!(
        test.sent(),
        vec![Command::List { location: Location::Session(session), path: Some(PathBuf::from("/home/user/child")) }]
    );
}

#[test]
fn refresh_relists_the_current_folder() {
    let mut test = app_in("/d");

    test.app.apply_action(Action::Refresh);

    assert_eq!(test.sent(), vec![Command::List { location: Location::Local, path: Some(PathBuf::from("/d")) }]);
}

#[test]
fn a_listing_of_a_new_local_folder_resets_the_cursor() {
    let mut test = app_in("/d");
    test.list_local(vec![entry(Path::new("/d"), "a", false), entry(Path::new("/d"), "b", false)]);
    test.app.local.cursor = 2;

    test.app.apply_core_event(Event::Listed {
        location: Location::Local,
        path: PathBuf::from("/d/sub"),
        entries: vec![entry(Path::new("/d/sub"), "x", false), entry(Path::new("/d/sub"), "y", false)],
    });

    assert_eq!(test.app.local.cursor, 0);
}

#[test]
fn a_failed_listing_leaves_the_panel_where_it_was() {
    let mut test = app_in("/d");
    test.list_local(vec![entry(Path::new("/d"), "child", true)]);
    test.app.local.cursor = 1;
    test.app.apply_action(Action::Open);

    test.app.apply_core_event(Event::Notice {
        severity: Severity::Error,
        message: "Permission denied (os error 13)".to_string(),
    });

    assert_eq!(test.app.local.path(), Path::new("/d"));
    assert_eq!(test.notification().as_deref(), Some("Permission denied (os error 13)"));
}

#[test]
fn back_action_returns_to_files_screen() {
    let mut test = app_in("/d");
    test.app.apply_action(Action::OpenConnections);
    test.app.apply_action(Action::Back);
    assert_eq!(test.app.screen, Screen::Files);
}

#[test]
fn connections_cursor_moves_within_bounds() {
    let mut test = app_in("/d");
    test.app.screen = Screen::Connections;
    test.app.connections = vec![connection("a", ConnectionSource::Profile), connection("b", ConnectionSource::Profile)];

    test.app.apply_action(Action::Up);
    assert_eq!(test.app.connections_cursor, 0);

    test.app.apply_action(Action::Down);
    assert_eq!(test.app.connections_cursor, 1);

    test.app.apply_action(Action::Down);
    assert_eq!(test.app.connections_cursor, 1);
}

#[test]
fn cycle_session_action_is_a_silent_no_op_with_no_sessions() {
    let mut test = app_in("/d");
    test.app.apply_action(Action::CycleSession);
    assert!(test.app.sessions.is_empty());
}

#[test]
fn cycle_session_action_advances_the_active_session() {
    let mut test = app_in("/d");
    let a = test.connect(1, "test");
    let b = test.connect(2, "other");
    assert_eq!(test.app.sessions.active().unwrap().id, b);

    test.app.apply_action(Action::CycleSession);

    assert_eq!(test.app.sessions.active().unwrap().id, a);
}

#[test]
fn delete_on_the_connections_screen_disconnects_the_selected_session() {
    let mut test = app_in("/d");
    let session = test.connect(5, "test");
    test.app.connections = vec![connection("test", ConnectionSource::Profile)];
    test.app.connections_cursor = 0;
    test.app.screen = Screen::Connections;

    test.app.apply_action(Action::Delete);
    test.app.apply_core_event(Event::Disconnected { session, name: "test".to_string() });

    assert!(test.sent().contains(&Command::Disconnect { session }));
    assert!(test.app.sessions.is_empty());
}

#[test]
fn delete_on_the_files_screen_still_opens_the_delete_dialog() {
    let mut test = app_in("/d");
    test.list_local(vec![entry(Path::new("/d"), "doomed.txt", false)]);
    test.app.local.cursor = 1;

    test.app.apply_action(Action::Delete);

    assert!(test.app.dialog.is_some());
}

#[test]
fn toggle_hidden_action_reveals_dotfiles_in_the_local_panel() {
    let mut test = app_in("/d");
    test.list_local(vec![entry(Path::new("/d"), ".secret", false)]);
    let before = test.app.local.rows().len();

    test.app.apply_action(Action::ToggleHidden);

    assert_eq!(test.app.local.rows().len(), before + 1);
}

#[test]
fn cycle_sort_action_changes_the_local_panel_sort_spec() {
    let mut test = app_in("/d");
    let before = test.app.local.sort_spec();

    test.app.apply_action(Action::CycleSort);

    assert_ne!(test.app.local.sort_spec(), before);
}

#[test]
fn back_action_dismisses_an_error_notification_before_changing_screens() {
    let mut test = app_in("/d");
    test.app.screen = Screen::Connections;
    test.app.notifications.push(Severity::Error, "oops");

    test.app.apply_action(Action::Back);

    assert!(test.app.notifications.current().is_none());
    assert_eq!(test.app.screen, Screen::Connections);
}

#[test]
fn back_action_returns_to_files_screen_when_there_is_no_error() {
    let mut test = app_in("/d");
    test.app.screen = Screen::Connections;

    test.app.apply_action(Action::Back);

    assert_eq!(test.app.screen, Screen::Files);
}

#[test]
fn rename_action_on_connections_screen_opens_the_edit_connection_form() {
    let mut test = app_in("/d");
    test.app.screen = Screen::Connections;
    let mut prod = connection("prod", ConnectionSource::Profile);
    prod.host = "server.example.com".to_string();
    prod.port = 2222;
    prod.username = "deploy".to_string();
    prod.password = Some("hunter2".to_string());
    test.app.connections = vec![prod];
    test.app.connections_cursor = 0;

    test.app.apply_action(Action::Rename);

    match test.app.dialog {
        Some(Dialog::Form(ref form)) => {
            assert_eq!(form.value("name").as_deref(), Some("prod"));
            assert_eq!(form.value("host").as_deref(), Some("server.example.com"));
            assert_eq!(form.value("port").as_deref(), Some("2222"));
            assert_eq!(form.value("username").as_deref(), Some("deploy"));
            assert_eq!(form.value("password").as_deref(), Some("hunter2"));
        }
        _ => panic!("expected the edit form to open"),
    }
}

#[test]
fn rename_action_on_an_ssh_config_entry_does_not_open_a_dialog() {
    let mut test = app_in("/d");
    test.app.screen = Screen::Connections;
    test.app.connections = vec![connection("prod", ConnectionSource::SshConfig)];
    test.app.connections_cursor = 0;

    test.app.apply_action(Action::Rename);

    assert!(test.app.dialog.is_none());
    assert!(test.app.notifications.current().is_some());
}

#[test]
fn f4_without_a_session_warns() {
    let mut test = app_in("/d");

    test.app.apply_action(Action::OpenTerminal);

    assert_eq!(test.notification().as_deref(), Some("Connect to a server first"));
    assert!(test.sent().is_empty());
}

#[test]
fn f4_on_a_connection_without_a_shell_warns_and_sends_nothing() {
    let mut test = app_in("/d");
    test.app.apply_core_event(Event::Connected { session: 1, name: "bucket".to_string(), shell_available: false });

    test.app.apply_action(Action::OpenTerminal);

    assert_eq!(test.notification().as_deref(), Some("This connection doesn't support a terminal session"));
    assert!(test.sent().is_empty());
}

#[test]
fn f4_on_a_connection_with_a_shell_asks_the_core_for_it() {
    let mut test = app_in("/d");
    let session = test.connect(8, "srv");

    test.app.apply_action(Action::OpenTerminal);

    assert_eq!(test.sent(), vec![Command::PrepareShell { session }]);
}
