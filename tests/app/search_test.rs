use super::*;
use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};
use std::fs;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn app_in_temp_dir() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::at(dir.path().to_path_buf()).unwrap();
    (dir, app)
}

#[test]
fn open_search_action_switches_to_the_search_screen_for_the_local_panel() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::OpenSearch);
    assert_eq!(app.screen, Screen::Search);
}

#[test]
fn open_search_action_warns_when_remote_is_focused_without_a_connection() {
    let (_dir, mut app) = app_in_temp_dir();
    app.active_panel = ActivePanel::Remote;

    app.apply_action(Action::OpenSearch);

    assert_eq!(app.screen, Screen::Files);
    assert!(app.notifications.current().is_some());
}

#[test]
fn esc_in_search_closes_the_screen() {
    let (_dir, mut app) = app_in_temp_dir();
    app.apply_action(Action::OpenSearch);

    app.apply_search_key(key(KeyCode::Esc));

    assert_eq!(app.screen, Screen::Files);
    assert!(app.search.is_none());
}

#[tokio::test]
async fn typing_a_pattern_streams_matching_results_back_into_the_search_view() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("target.log"), b"x").unwrap();
    let mut app = App::at(dir.path().to_path_buf()).unwrap();

    app.apply_action(Action::OpenSearch);
    for c in "target".chars() {
        app.apply_search_key(key(KeyCode::Char(c)));
    }

    tokio::time::sleep(SEARCH_DEBOUNCE + std::time::Duration::from_millis(200)).await;
    while let Ok(event) = app.search_rx.try_recv() {
        app.apply_search_event(event);
    }

    assert!(app.search.unwrap().view.results.iter().any(|entry| entry.name == "target.log"));
}

#[tokio::test]
async fn rapid_pattern_changes_dispatch_only_one_search_for_the_final_pattern() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("aaa.log"), b"x").unwrap();
    fs::write(dir.path().join("bbb.log"), b"x").unwrap();
    let mut app = App::at(dir.path().to_path_buf()).unwrap();

    app.apply_action(Action::OpenSearch);
    for c in "aaa".chars() {
        app.apply_search_key(key(KeyCode::Char(c)));
    }

    tokio::time::sleep(SEARCH_DEBOUNCE + std::time::Duration::from_millis(200)).await;
    let mut done_count = 0;
    while let Ok(event) = app.search_rx.try_recv() {
        if matches!(event, SearchEvent::Done { .. }) {
            done_count += 1;
        }
        app.apply_search_event(event);
    }

    assert_eq!(done_count, 1, "expected exactly one dispatched search");
    let session = app.search.unwrap();
    assert!(session.view.results.iter().any(|entry| entry.name == "aaa.log"));
    assert!(!session.view.results.iter().any(|entry| entry.name == "bbb.log"));
}
