use std::path::Path;

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::{
    testing::{entry, test_app},
    *,
};

#[test]
fn at_with_applies_panel_settings_to_the_local_panel() {
    let (core, _commands) = CoreHandle::detached();
    let settings = PanelSettings { show_hidden: true, sort: Default::default() };
    let mut app = App::new(core, PathBuf::from("/d"), &settings, input::KeyBindings::defaults());

    app.apply_core_event(Event::Listed {
        location: Location::Local,
        path: PathBuf::from("/d"),
        entries: vec![entry(Path::new("/d"), ".hidden", false)],
    });

    assert_eq!(app.local.rows().len(), 2);
}

#[test]
fn starting_asks_the_core_for_the_local_listing() {
    let (core, mut commands) = CoreHandle::detached();

    App::new(core, PathBuf::from("/start"), &PanelSettings::default(), input::KeyBindings::defaults());

    assert_eq!(
        commands.try_recv().unwrap(),
        Command::List { location: Location::Local, path: Some(PathBuf::from("/start")) }
    );
}

#[test]
fn key_bindings_from_config_are_used_for_key_mapping() {
    let mut test = test_app(Path::new("/d"));
    let mut overrides = std::collections::HashMap::new();
    overrides.insert("quit".to_string(), "ctrl+q".to_string());
    let (bindings, _) = input::KeyBindings::from_overrides(&overrides);
    test.app.key_bindings = bindings;

    let event = KeyEvent {
        code: KeyCode::Char('q'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };

    assert_eq!(test.app.key_bindings.map_key(event), Action::Quit);
}

#[test]
fn set_status_pushes_an_error_notification_on_failure() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_core_event(Event::Notice { severity: Severity::Error, message: "boom".to_string() });

    let current = test.app.notifications.current().unwrap();
    assert_eq!(current.message, "boom");
    assert_eq!(current.severity, Severity::Error);
}

#[test]
fn set_status_does_nothing_on_success() {
    let mut test = test_app(Path::new("/d"));

    test.app.apply_core_event(Event::LocationChanged { location: Location::Local });

    assert!(test.app.notifications.current().is_none());
}

#[test]
fn a_location_change_relists_what_the_panel_shows() {
    let mut test = test_app(Path::new("/d"));
    let session = test.connect(3, "srv");
    test.list_remote(session, "/srv/www", Vec::new());

    test.app.apply_core_event(Event::LocationChanged { location: Location::Session(session) });
    test.app.apply_core_event(Event::LocationChanged { location: Location::Local });

    assert_eq!(
        test.sent(),
        vec![
            Command::List { location: Location::Session(session), path: Some(PathBuf::from("/srv/www")) },
            Command::List { location: Location::Local, path: Some(PathBuf::from("/d")) },
        ]
    );
}
