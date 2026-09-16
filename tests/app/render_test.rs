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
fn failed_status_does_not_clobber_the_title_when_a_session_is_active() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (_dir, mut app) = app_in_temp_dir();
    app.sessions.insert(
        sample_connection_entry(),
        PanelState::from_listing(PathBuf::from("/"), Vec::new()),
    );
    app.connection_status = ConnectionStatus::Failed("boom".to_string());

    let backend = TestBackend::new(60, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render_title(frame, frame.area()))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(content.contains("SSH: Connected"));
    assert!(!content.contains("Connection failed"));
}

#[test]
fn failed_status_still_shows_when_there_is_no_active_session() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (_dir, mut app) = app_in_temp_dir();
    app.connection_status = ConnectionStatus::Failed("boom".to_string());

    let backend = TestBackend::new(60, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| app.render_title(frame, frame.area()))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(content.contains("Connection failed"));
}
