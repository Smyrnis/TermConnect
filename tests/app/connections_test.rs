use crossterm::event::KeyEventState;

use super::*;
use crate::connection::ConnectionSource;

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

static HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn app_with_isolated_home() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>, App) {
    let guard = HOME_ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", dir.path());
    }
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, guard, app)
}

fn sample_connection_entry() -> ConnectionEntry {
    ConnectionEntry {
        name: "test".to_string(),
        host: "test.example.com".to_string(),
        port: 22,
        username: "user".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }
}

const PORT_NOTHING_LISTENS_ON: u16 = 1;
#[test]
fn open_connections_switches_screen_and_loads_entries() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::OpenConnections);
    assert_eq!(app.screen, Screen::Connections);
}

#[tokio::test]
async fn connecting_to_an_unreachable_host_reports_failure() {
    let (_dir, mut app) = app_in_temp_dir();
    app.screen = Screen::Connections;
    app.connections = vec![ConnectionEntry {
        name: "unreachable".to_string(),
        host: "127.0.0.1".to_string(),
        port: PORT_NOTHING_LISTENS_ON,
        username: "user".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }];

    app.connect_to_selected();
    assert_eq!(app.connection_status, ConnectionStatus::Connecting("unreachable".to_string()));

    let event = app.connect_rx.recv().await.unwrap();
    app.apply_connect_event(event);

    match app.connection_status {
        ConnectionStatus::Failed(_) => {}
        ref other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn panel_event_listed_updates_the_matching_sessions_panel() {
    let (_dir, mut app) = app_in_temp_dir();
    let id = app.sessions.insert(sample_connection_entry(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));

    app.apply_panel_event(PanelEvent::Listed {
        session_id: id,
        path: PathBuf::from("/home/user"),
        entries: Vec::new(),
    });

    assert_eq!(app.sessions.active().unwrap().panel.path(), std::path::Path::new("/home/user"));
}

#[test]
fn panel_event_listed_for_a_vanished_session_is_dropped() {
    let (_dir, mut app) = app_in_temp_dir();

    app.apply_panel_event(PanelEvent::Listed { session_id: 999, path: PathBuf::from("/x"), entries: Vec::new() });

    assert!(app.sessions.is_empty());
}

#[test]
fn panel_event_failed_shows_a_notification_when_the_session_still_exists() {
    let (_dir, mut app) = app_in_temp_dir();
    let id = app.sessions.insert(sample_connection_entry(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));

    app.apply_panel_event(PanelEvent::Failed { session_id: id, message: "boom".to_string() });

    assert_eq!(app.notifications.current().unwrap().message, "boom");
}

#[test]
fn panel_event_failed_for_a_vanished_session_is_dropped() {
    let (_dir, mut app) = app_in_temp_dir();

    app.apply_panel_event(PanelEvent::Failed { session_id: 999, message: "boom".to_string() });

    assert!(app.notifications.current().is_none());
}

#[test]
fn connecting_to_an_already_connected_host_switches_instead_of_reconnecting() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    app.sessions.insert(entry.clone(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));
    app.connections = vec![entry];
    app.connections_cursor = 0;

    app.connect_to_selected();

    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.connection_status, ConnectionStatus::Disconnected);
}

#[test]
fn disconnect_selected_reports_a_confirmation_notification() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    app.sessions.insert(entry.clone(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));
    app.connections = vec![entry];
    app.connections_cursor = 0;
    app.screen = Screen::Connections;

    app.apply_action(Action::Delete);

    let messages: Vec<String> = std::iter::from_fn(|| {
        let message = app.notifications.current().map(|n| n.message.clone());
        if message.is_some() {
            app.notifications.dismiss_current();
        }
        message
    })
    .collect();

    assert!(
        messages.iter().any(|m| m.contains("Disconnected from")),
        "expected a disconnect confirmation notification, got {messages:?}"
    );
}

#[test]
fn disconnecting_a_session_fails_its_queued_jobs_with_one_aggregated_notification() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    let id = app.sessions.insert(entry.clone(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));
    app.connections = vec![entry];
    app.connections_cursor = 0;
    app.screen = Screen::Connections;

    let job_a = app.transfers.enqueue(
        id,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        10,
        None,
    );
    let job_b = app.transfers.enqueue(
        id,
        Direction::Upload,
        PathBuf::from("/local/b.txt"),
        "/remote/b.txt".to_string(),
        "b.txt".to_string(),
        10,
        None,
    );
    let other_session_job = app.transfers.enqueue(
        999,
        Direction::Upload,
        PathBuf::from("/local/c.txt"),
        "/remote/c.txt".to_string(),
        "c.txt".to_string(),
        10,
        None,
    );

    app.disconnect_selected();

    assert!(matches!(app.transfers.get(job_a).unwrap().status, JobStatus::Failed(_)));
    assert!(matches!(app.transfers.get(job_b).unwrap().status, JobStatus::Failed(_)));
    assert_eq!(app.transfers.get(other_session_job).unwrap().status, JobStatus::Queued);

    let messages: Vec<String> = std::iter::from_fn(|| {
        let message = app.notifications.current().map(|n| n.message.clone());
        if message.is_some() {
            app.notifications.dismiss_current();
        }
        message
    })
    .collect();

    let transfer_messages: Vec<&String> =
        messages.iter().filter(|m| m.contains("transfer") && m.contains("disconnected")).collect();
    assert_eq!(transfer_messages.len(), 1, "expected exactly one aggregated transfer notification, got {messages:?}");
    assert!(transfer_messages[0].contains("2"));
}

#[test]
fn disconnecting_a_session_counts_its_active_jobs_in_the_aggregated_notification() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    let id = app.sessions.insert(entry.clone(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));
    app.connections = vec![entry];
    app.connections_cursor = 0;
    app.screen = Screen::Connections;
    let active = app.transfers.enqueue(
        id,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        10,
        None,
    );
    app.transfers.get_mut(active).unwrap().status = JobStatus::InProgress;
    let cancel = Arc::new(AtomicBool::new(false));
    app.transfer_cancels.insert(active, cancel.clone());
    app.transfers.enqueue(
        id,
        Direction::Upload,
        PathBuf::from("/local/b.txt"),
        "/remote/b.txt".to_string(),
        "b.txt".to_string(),
        10,
        None,
    );

    app.disconnect_selected();

    assert!(cancel.load(Ordering::Relaxed));
    let first = app.notifications.current().unwrap().message.clone();
    assert!(first.contains("2 transfers cancelled"), "got {first}");
}

#[test]
fn disconnecting_a_session_counts_its_scans_in_the_aggregated_notification() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    let id = app.sessions.insert(entry.clone(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));
    app.connections = vec![entry];
    app.connections_cursor = 0;
    app.screen = Screen::Connections;
    let scan_cancel = Arc::new(AtomicBool::new(false));
    app.planning.push(PlanningScan {
        batch_id: 0,
        session_id: id,
        direction: Direction::Upload,
        display_name: "myfolder".to_string(),
        cancel: scan_cancel.clone(),
    });
    app.transfers.enqueue(
        id,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        10,
        None,
    );

    app.disconnect_selected();

    assert!(scan_cancel.load(Ordering::Relaxed));
    let first = app.notifications.current().unwrap().message.clone();
    assert!(first.contains("2 transfers cancelled"), "got {first}");
}

#[test]
fn add_connection_dialog_saves_a_new_profile_on_submit() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    app.screen = Screen::Connections;

    app.apply_action(Action::AddConnection);
    assert!(matches!(app.dialog, Some(Dialog::Form(_))));

    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[0].value = "prod".to_string();
        form.fields[1].value = "server.example.com".to_string();
        form.fields[2].value = "2222".to_string();
        form.fields[3].value = "deploy".to_string();
        form.fields[4].value = "hunter2".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    assert!(app.dialog.is_none());
    let saved = crate::connection::store::load().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "prod");
    assert_eq!(saved[0].host, "server.example.com");
    assert_eq!(saved[0].port, 2222);
    assert_eq!(saved[0].password, Some("hunter2".to_string()));
}

#[test]
fn add_connection_dialog_keeps_the_dialog_open_on_invalid_port() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    app.screen = Screen::Connections;

    app.apply_action(Action::AddConnection);
    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[0].value = "prod".to_string();
        form.fields[1].value = "server.example.com".to_string();
        form.fields[2].value = "not-a-port".to_string();
        form.fields[3].value = "deploy".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    match app.dialog {
        Some(Dialog::Form(ref form)) => assert!(form.error.is_some()),
        _ => panic!("expected the form dialog to stay open with an error"),
    }
}

#[test]
fn add_connection_dialog_keeps_the_dialog_open_on_port_zero() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    app.screen = Screen::Connections;

    app.apply_action(Action::AddConnection);
    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[0].value = "prod".to_string();
        form.fields[1].value = "server.example.com".to_string();
        form.fields[2].value = "0".to_string();
        form.fields[3].value = "deploy".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    match app.dialog {
        Some(Dialog::Form(ref form)) => assert!(form.error.is_some()),
        _ => panic!("expected the form dialog to stay open with an error"),
    }
}

#[test]
fn add_connection_dialog_rejects_a_name_that_already_exists() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    crate::connection::store::save(&crate::connection::profile::ConnectionProfile {
        name: "prod".to_string(),
        host: "original.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
    })
    .unwrap();

    app.screen = Screen::Connections;
    app.apply_action(Action::AddConnection);
    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[0].value = "prod".to_string();
        form.fields[1].value = "new.example.com".to_string();
        form.fields[2].value = "22".to_string();
        form.fields[3].value = "deploy".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    match app.dialog {
        Some(Dialog::Form(ref form)) => assert!(form.error.is_some()),
        _ => panic!("expected the form dialog to stay open with an error"),
    }

    let saved = crate::connection::store::load().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].host, "original.example.com");
}

#[test]
fn edit_connection_rejects_renaming_onto_an_existing_name() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    crate::connection::store::save(&crate::connection::profile::ConnectionProfile {
        name: "prod".to_string(),
        host: "prod.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
    })
    .unwrap();
    crate::connection::store::save(&crate::connection::profile::ConnectionProfile {
        name: "staging".to_string(),
        host: "staging.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
    })
    .unwrap();

    app.screen = Screen::Connections;
    app.connections = vec![ConnectionEntry {
        name: "prod".to_string(),
        host: "prod.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }];
    app.connections_cursor = 0;

    app.apply_action(Action::Rename);
    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[0].value = "staging".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    match app.dialog {
        Some(Dialog::Form(ref form)) => assert!(form.error.is_some()),
        _ => panic!("expected the form dialog to stay open with an error"),
    }

    let mut saved = crate::connection::store::load().unwrap();
    saved.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].name, "prod");
    assert_eq!(saved[0].host, "prod.example.com");
    assert_eq!(saved[1].name, "staging");
    assert_eq!(saved[1].host, "staging.example.com");
}

#[test]
fn add_connection_action_does_nothing_outside_the_connections_screen() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::AddConnection);
    assert!(app.dialog.is_none());
}

#[test]
fn add_connection_dialog_stays_open_when_saving_fails() {
    let (dir, _guard, mut app) = app_with_isolated_home();
    std::fs::write(dir.path().join(".config"), b"not a directory").unwrap();
    app.screen = Screen::Connections;

    app.apply_action(Action::AddConnection);
    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[0].value = "prod".to_string();
        form.fields[1].value = "server.example.com".to_string();
        form.fields[2].value = "22".to_string();
        form.fields[3].value = "deploy".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    assert!(app.dialog.is_some(), "the form must stay open so the user's input isn't lost on a save error");
    assert!(app.notifications.current().is_some());
}

#[test]
fn edit_connection_preserves_identity_file_and_remote_path_not_shown_in_the_form() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    crate::connection::store::save(&crate::connection::profile::ConnectionProfile {
        name: "prod".to_string(),
        host: "old.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: Some(std::path::PathBuf::from("/home/user/.ssh/id_ed25519")),
        remote_path: Some("/var/www".to_string()),
        password: None,
    })
    .unwrap();

    app.screen = Screen::Connections;
    app.connections = vec![ConnectionEntry {
        name: "prod".to_string(),
        host: "old.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: Some(std::path::PathBuf::from("/home/user/.ssh/id_ed25519")),
        remote_path: Some("/var/www".to_string()),
        password: None,
        source: ConnectionSource::Profile,
    }];
    app.connections_cursor = 0;

    app.apply_action(Action::Rename);
    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[1].value = "new.example.com".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    let saved = crate::connection::store::load().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].host, "new.example.com");
    assert_eq!(saved[0].identity_file, Some(std::path::PathBuf::from("/home/user/.ssh/id_ed25519")));
    assert_eq!(saved[0].remote_path, Some("/var/www".to_string()));
}

#[test]
fn renaming_a_connection_to_a_new_name_deletes_the_old_profile() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    crate::connection::store::save(&crate::connection::profile::ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
    })
    .unwrap();

    app.screen = Screen::Connections;
    app.connections = vec![ConnectionEntry {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }];
    app.connections_cursor = 0;

    app.apply_action(Action::Rename);
    if let Some(Dialog::Form(form)) = app.dialog.as_mut() {
        form.fields[0].value = "production".to_string();
    }
    app.apply_dialog_key(key(KeyCode::Enter));

    let saved = crate::connection::store::load().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "production");
}

#[test]
fn delete_connection_action_opens_a_confirm_dialog() {
    let (_dir, mut app) = app_in_temp_dir();
    app.screen = Screen::Connections;
    app.connections = vec![ConnectionEntry {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }];
    app.connections_cursor = 0;

    app.apply_action(Action::DeleteConnection);

    assert!(matches!(app.dialog, Some(Dialog::Confirm(_))));
}

#[test]
fn confirming_delete_connection_removes_the_saved_profile() {
    let (_dir, _guard, mut app) = app_with_isolated_home();
    crate::connection::store::save(&crate::connection::profile::ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
    })
    .unwrap();

    app.screen = Screen::Connections;
    app.connections = vec![ConnectionEntry {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }];
    app.connections_cursor = 0;

    app.apply_action(Action::DeleteConnection);
    app.apply_dialog_key(key(KeyCode::Char('y')));

    assert!(app.dialog.is_none());
    assert!(crate::connection::store::load().unwrap().is_empty());
}

#[test]
fn delete_connection_on_an_ssh_config_entry_does_not_open_a_dialog() {
    let (_dir, mut app) = app_in_temp_dir();
    app.screen = Screen::Connections;
    app.connections = vec![ConnectionEntry {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::SshConfig,
    }];
    app.connections_cursor = 0;

    app.apply_action(Action::DeleteConnection);

    assert!(app.dialog.is_none());
    assert!(app.notifications.current().is_some());
}
