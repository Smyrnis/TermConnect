use super::*;
use crate::connection::ConnectionSource;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

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
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }
}

fn planning_scan(batch_id: u64, name: &str) -> PlanningScan {
    PlanningScan { batch_id, display_name: name.to_string(), cancel: Arc::new(AtomicBool::new(false)) }
}

fn render_status_text(app: &App) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(60, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render_status(frame, frame.area())).unwrap();
    terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn failed_status_does_not_clobber_the_title_when_a_session_is_active() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (_dir, mut app) = app_in_temp_dir();
    app.sessions.insert(sample_connection_entry(), PanelState::from_listing(PathBuf::from("/"), Vec::new()));
    app.connection_status = ConnectionStatus::Failed("boom".to_string());

    let backend = TestBackend::new(60, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render_title(frame, frame.area())).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

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
    terminal.draw(|frame| app.render_title(frame, frame.area())).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("Connection failed"));
}

#[test]
fn render_status_shows_scanning_while_planning() {
    let (_dir, mut app) = app_in_temp_dir();
    app.planning.push(planning_scan(1, "myfolder"));

    assert!(render_status_text(&app).contains("Scanning myfolder"));
}

#[test]
fn transfer_status_text_shows_batch_progress_for_a_batch_job() {
    let (_dir, mut app) = app_in_temp_dir();
    let batch_id = app.transfers.start_batch();
    let active = app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        100,
        Some(batch_id),
    );
    app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/b.txt"),
        "/remote/b.txt".to_string(),
        "b.txt".to_string(),
        100,
        Some(batch_id),
    );
    {
        let job = app.transfers.get_mut(active).unwrap();
        job.status = JobStatus::InProgress;
        job.transferred_bytes = 50;
    }

    let text = render_status_text(&app);

    assert!(text.contains("0/2 files"));
    assert!(text.contains("25%"));
}

#[test]
fn transfer_status_text_is_unchanged_for_a_non_batch_job() {
    let (_dir, mut app) = app_in_temp_dir();
    let active = app.transfers.enqueue(
        1,
        Direction::Upload,
        PathBuf::from("/local/a.txt"),
        "/remote/a.txt".to_string(),
        "a.txt".to_string(),
        100,
        None,
    );
    {
        let job = app.transfers.get_mut(active).unwrap();
        job.status = JobStatus::InProgress;
        job.transferred_bytes = 50;
    }

    let text = render_status_text(&app);

    assert!(text.contains("a.txt: 50%"));
    assert!(!text.contains("files"));
}

#[test]
fn planning_status_text_is_none_without_a_scan() {
    let (_dir, app) = app_in_temp_dir();

    assert_eq!(app.planning_status_text(), None);
}

#[test]
fn planning_status_text_names_a_single_scan() {
    let (_dir, mut app) = app_in_temp_dir();
    app.planning.push(planning_scan(1, "myfolder"));

    assert_eq!(app.planning_status_text(), Some("Scanning myfolder\u{2026}".to_string()));
}

#[test]
fn planning_status_text_counts_several_scans() {
    let (_dir, mut app) = app_in_temp_dir();
    app.planning.push(planning_scan(1, "one"));
    app.planning.push(planning_scan(2, "two"));

    assert_eq!(app.planning_status_text(), Some("Scanning 2 copies\u{2026}".to_string()));
}
