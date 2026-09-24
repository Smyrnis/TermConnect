use std::path::Path;

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::*;
use crate::app::testing::{entry, test_app};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

#[test]
fn open_search_action_switches_to_the_search_screen_for_the_local_panel() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenSearch);
    assert_eq!(test.app.screen, Screen::Search);
}

#[test]
fn open_search_action_warns_when_remote_is_focused_without_a_connection() {
    let mut test = test_app(Path::new("/d"));
    test.app.active_panel = ActivePanel::Remote;

    test.app.apply_action(Action::OpenSearch);

    assert_eq!(test.app.screen, Screen::Files);
    assert!(test.app.notifications.current().is_some());
}

#[test]
fn esc_in_search_closes_the_screen() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenSearch);

    test.app.apply_search_key(key(KeyCode::Esc));

    assert_eq!(test.app.screen, Screen::Files);
    assert!(test.app.search.is_none());
    assert_eq!(test.sent(), vec![Command::CancelSearch]);
}

#[test]
fn typing_a_pattern_streams_matching_results_back_into_the_search_view() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenSearch);

    for c in "target".chars() {
        test.app.apply_search_key(key(KeyCode::Char(c)));
    }
    test.app.apply_core_event(Event::SearchFound(entry(Path::new("/d"), "target.log", false)));
    test.app.apply_core_event(Event::SearchDone { truncated: false });

    assert!(test.app.search.as_ref().unwrap().view.results.iter().any(|entry| entry.name == "target.log"));
    assert_eq!(
        test.sent().last(),
        Some(&Command::Search {
            location: Location::Local,
            root: PathBuf::from("/d"),
            pattern: "*target*".to_string()
        })
    );
}

#[test]
fn rapid_pattern_changes_dispatch_only_one_search_for_the_final_pattern() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenSearch);

    for c in "aaa".chars() {
        test.app.apply_search_key(key(KeyCode::Char(c)));
    }

    let searches: Vec<String> = test
        .sent()
        .into_iter()
        .filter_map(|command| match command {
            Command::Search { pattern, .. } => Some(pattern),
            _ => None,
        })
        .collect();
    assert_eq!(searches.last().map(String::as_str), Some("*aaa*"), "the core debounces all but the last");
}

#[test]
fn clearing_the_pattern_cancels_the_search() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenSearch);
    test.app.apply_search_key(key(KeyCode::Char('a')));
    test.sent();

    test.app.apply_search_key(key(KeyCode::Backspace));

    assert_eq!(test.sent(), vec![Command::CancelSearch]);
}

#[test]
fn ctrl_c_cancels_without_leaving_the_screen() {
    let mut test = test_app(Path::new("/d"));
    test.app.apply_action(Action::OpenSearch);

    test.app.apply_search_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert_eq!(test.app.screen, Screen::Search);
    assert_eq!(test.sent(), vec![Command::CancelSearch]);
}

#[test]
fn opening_a_result_lists_its_folder_in_the_right_panel() {
    let mut test = test_app(Path::new("/d"));
    let session = test.connect(3, "srv");
    test.list_remote(session, "/srv", Vec::new());
    test.app.active_panel = ActivePanel::Remote;
    test.app.apply_action(Action::OpenSearch);
    test.app.apply_core_event(Event::SearchFound(entry(Path::new("/srv/logs"), "x.log", false)));
    test.sent();

    test.app.apply_search_key(key(KeyCode::Enter));

    assert_eq!(test.app.screen, Screen::Files);
    assert_eq!(
        test.sent(),
        vec![
            Command::CancelSearch,
            Command::List { location: Location::Session(session), path: Some(PathBuf::from("/srv/logs")) },
        ]
    );
}
