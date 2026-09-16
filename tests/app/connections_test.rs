use super::*;

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
        port: 1, // nothing listens on port 1
        username: "user".to_string(),
        identity_file: None,
    }];

    app.connect_to_selected();
    assert_eq!(
        app.connection_status,
        ConnectionStatus::Connecting("unreachable".to_string())
    );

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
    let id = app.sessions.insert(
        sample_connection_entry(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );

    app.apply_panel_event(PanelEvent::Listed {
        session_id: id,
        path: PathBuf::from("/home/user"),
        entries: Vec::new(),
    });

    assert_eq!(
        app.sessions.active().unwrap().panel.path(),
        std::path::Path::new("/home/user")
    );
}

#[test]
fn panel_event_listed_for_a_vanished_session_is_dropped() {
    let (_dir, mut app) = app_in_temp_dir();

    app.apply_panel_event(PanelEvent::Listed {
        session_id: 999,
        path: PathBuf::from("/x"),
        entries: Vec::new(),
    });

    assert!(app.sessions.is_empty());
}

#[test]
fn panel_event_failed_shows_a_notification_when_the_session_still_exists() {
    let (_dir, mut app) = app_in_temp_dir();
    let id = app.sessions.insert(
        sample_connection_entry(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );

    app.apply_panel_event(PanelEvent::Failed {
        session_id: id,
        message: "boom".to_string(),
    });

    assert_eq!(app.notifications.current().unwrap().message, "boom");
}

#[test]
fn panel_event_failed_for_a_vanished_session_is_dropped() {
    let (_dir, mut app) = app_in_temp_dir();

    app.apply_panel_event(PanelEvent::Failed {
        session_id: 999,
        message: "boom".to_string(),
    });

    assert!(app.notifications.current().is_none());
}

#[test]
fn connecting_to_an_already_connected_host_switches_instead_of_reconnecting() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    app.sessions.insert(
        entry.clone(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );
    app.connections = vec![entry];
    app.connections_cursor = 0;

    app.connect_to_selected();

    // still exactly one session — no reconnect attempt was spawned
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.connection_status, ConnectionStatus::Disconnected);
}

#[test]
fn disconnect_selected_reports_a_confirmation_notification() {
    let (_dir, mut app) = app_in_temp_dir();
    let entry = sample_connection_entry();
    app.sessions.insert(
        entry.clone(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );
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
    let id = app.sessions.insert(
        entry.clone(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );
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
    );
    let job_b = app.transfers.enqueue(
        id,
        Direction::Upload,
        PathBuf::from("/local/b.txt"),
        "/remote/b.txt".to_string(),
        "b.txt".to_string(),
        10,
    );
    // A job for a different session should be untouched.
    let other_session_job = app.transfers.enqueue(
        999,
        Direction::Upload,
        PathBuf::from("/local/c.txt"),
        "/remote/c.txt".to_string(),
        "c.txt".to_string(),
        10,
    );

    app.disconnect_selected();

    assert!(matches!(
        app.transfers.get(job_a).unwrap().status,
        JobStatus::Failed(_)
    ));
    assert!(matches!(
        app.transfers.get(job_b).unwrap().status,
        JobStatus::Failed(_)
    ));
    assert_eq!(
        app.transfers.get(other_session_job).unwrap().status,
        JobStatus::Queued
    );

    // Collect every notification pushed, in order.
    let messages: Vec<String> = std::iter::from_fn(|| {
        let message = app.notifications.current().map(|n| n.message.clone());
        if message.is_some() {
            app.notifications.dismiss_current();
        }
        message
    })
    .collect();

    // Exactly one message aggregates both cancelled/failed transfers —
    // not one notification per job — plus the disconnect confirmation.
    let transfer_messages: Vec<&String> = messages
        .iter()
        .filter(|m| m.contains("transfer") && m.contains("disconnected"))
        .collect();
    assert_eq!(
        transfer_messages.len(),
        1,
        "expected exactly one aggregated transfer notification, got {messages:?}"
    );
    assert!(transfer_messages[0].contains("2"));
}
