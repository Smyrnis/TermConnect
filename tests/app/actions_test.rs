use super::*;
use std::fs;

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

fn sample_connection_entry() -> ConnectionEntry {
    ConnectionEntry {
        name: "test".to_string(),
        host: "test.example.com".to_string(),
        port: 22,
        username: "user".to_string(),
        identity_file: None,
    }
}

#[test]
fn quit_action_sets_should_quit() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::Quit);
    assert!(app.should_quit);
}

#[test]
fn help_action_shows_the_overlay() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::Help);
    assert!(app.help_visible);
}

#[test]
fn switch_panel_action_toggles_active_panel() {
    let (_dir, mut app) = app_in_temp_dir();
    assert_eq!(app.active_panel, ActivePanel::Local);
    app.apply_action(Action::SwitchPanel);
    assert_eq!(app.active_panel, ActivePanel::Remote);
}

#[test]
fn noop_action_does_not_change_state() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::Noop);
    assert!(!app.should_quit);
    assert_eq!(app.active_panel, ActivePanel::Local);
}

#[test]
fn panel_actions_are_ignored_when_remote_panel_has_no_listing_yet() {
    let (dir, mut app) = app_in_temp_dir();
    fs::create_dir(dir.path().join("child")).unwrap();
    app.apply_action(Action::SwitchPanel);

    let cursor_before = app.local.cursor;
    app.apply_action(Action::Down);

    assert_eq!(app.local.cursor, cursor_before);
}

#[test]
fn remote_panel_navigation_works_once_a_listing_exists() {
    let (_dir, mut app) = app_in_temp_dir();
    let panel = PanelState::from_listing(
        PathBuf::from("/home/user"),
        vec![Entry {
            name: "child".to_string(),
            path: PathBuf::from("/home/user/child"),
            is_dir: true,
            size: 0,
            permissions: None,
        }],
    );
    app.sessions.insert(sample_connection_entry(), panel);
    app.active_panel = ActivePanel::Remote;

    app.apply_action(Action::Down);

    assert_eq!(app.sessions.active().unwrap().panel.cursor, 1);
}

#[test]
fn back_action_returns_to_files_screen() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::OpenConnections);
    app.apply_action(Action::Back);
    assert_eq!(app.screen, Screen::Files);
}

#[test]
fn connections_cursor_moves_within_bounds() {
    let (_dir, mut app) = app_in_temp_dir();
    app.screen = Screen::Connections;
    app.connections = vec![
        ConnectionEntry {
            name: "a".to_string(),
            host: "a.example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            identity_file: None,
        },
        ConnectionEntry {
            name: "b".to_string(),
            host: "b.example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            identity_file: None,
        },
    ];

    app.apply_action(Action::Up);
    assert_eq!(app.connections_cursor, 0);

    app.apply_action(Action::Down);
    assert_eq!(app.connections_cursor, 1);

    app.apply_action(Action::Down);
    assert_eq!(app.connections_cursor, 1);
}

#[test]
fn cycle_session_action_is_a_silent_no_op_with_no_sessions() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::CycleSession);
    assert!(app.sessions.is_empty());
}

#[test]
fn cycle_session_action_advances_the_active_session() {
    let (_dir, mut app) = app_in_temp_dir();
    let a = app.sessions.insert(
        sample_connection_entry(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );
    let mut second_entry = sample_connection_entry();
    second_entry.name = "other".to_string();
    let b = app.sessions.insert(
        second_entry,
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );
    assert_eq!(app.sessions.active().unwrap().id, b);

    app.apply_action(Action::CycleSession);

    assert_eq!(app.sessions.active().unwrap().id, a);
}

#[test]
fn delete_on_the_connections_screen_disconnects_the_selected_session() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    app.sessions.insert(
        entry.clone(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );
    app.connections = vec![entry];
    app.connections_cursor = 0;
    app.screen = Screen::Connections;

    assert_eq!(app.sessions.len(), 1);

    app.apply_action(Action::Delete);

    // The session (and any resources held for it) are gone — this is
    // the only way a connected session can ever be closed, since
    // `Sessions`/`SessionResources` can't be constructed with a live
    // handle in a unit test (see `SessionResources`'s doc comment), so
    // `session_resources` itself starts and stays empty here; the bug
    // this guards against is `Action::Delete` never reaching
    // `disconnect_selected` at all (it was intercepted earlier by the
    // mkdir/delete-dialog arm regardless of screen).
    assert!(app.sessions.is_empty());
    assert!(app.session_resources.is_empty());
}

#[test]
fn delete_on_the_files_screen_still_opens_the_delete_dialog() {
    let (dir, mut app) = app_in_temp_dir();
    fs::write(dir.path().join("doomed.txt"), b"content").unwrap();
    app.local.refresh().unwrap();
    app.local.cursor = app.local.rows().len() - 1;

    app.apply_action(Action::Delete);

    assert!(app.dialog.is_some());
}

#[test]
fn toggle_hidden_action_reveals_dotfiles_in_the_local_panel() {
    let (dir, mut app) = app_in_temp_dir();
    fs::write(dir.path().join(".secret"), b"x").unwrap();
    app.local.refresh().unwrap();
    let before = app.local.rows().len();

    app.apply_action(Action::ToggleHidden);

    assert_eq!(app.local.rows().len(), before + 1);
}

#[test]
fn cycle_sort_action_changes_the_local_panel_sort_spec() {
    let (_dir, mut app) = app_in_temp_dir();
    let before = app.local.sort_spec();

    app.apply_action(Action::CycleSort);

    assert_ne!(app.local.sort_spec(), before);
}

#[test]
fn back_action_dismisses_an_error_notification_before_changing_screens() {
    let (_dir, mut app) = app_in_temp_dir();
    app.screen = Screen::Connections;
    app.notifications.push(Severity::Error, "oops");

    app.apply_action(Action::Back);

    assert!(app.notifications.current().is_none());
    assert_eq!(app.screen, Screen::Connections);
}

#[test]
fn back_action_returns_to_files_screen_when_there_is_no_error() {
    let (_dir, mut app) = app_in_temp_dir();
    app.screen = Screen::Connections;

    app.apply_action(Action::Back);

    assert_eq!(app.screen, Screen::Files);
}
