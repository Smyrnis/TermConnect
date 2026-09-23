use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    key_with_modifiers(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn map_key_via_defaults(code: KeyCode) -> Action {
    KeyBindings::defaults().map_key(key(code))
}

fn map_key_via_defaults_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> Action {
    KeyBindings::defaults().map_key(key_with_modifiers(code, modifiers))
}

#[test]
fn format_key_spec_renders_a_plain_function_key() {
    assert_eq!(format_key_spec(KeySpec { code: KeyCode::F(10), modifiers: KeyModifiers::NONE }), "F10");
}

#[test]
fn format_key_spec_renders_a_control_modifier() {
    assert_eq!(format_key_spec(KeySpec { code: KeyCode::Char('r'), modifiers: KeyModifiers::CONTROL }), "Ctrl+R");
}

#[test]
fn defaults_cover_every_bindable_action_with_the_original_hardcoded_keys() {
    let bindings = KeyBindings::defaults();
    let cases = [
        (Action::Quit, key(KeyCode::F(10))),
        (Action::SwitchPanel, key(KeyCode::Tab)),
        (Action::Up, key(KeyCode::Up)),
        (Action::Down, key(KeyCode::Down)),
        (Action::Open, key(KeyCode::Enter)),
        (Action::ToggleSelect, key(KeyCode::Char(' '))),
        (Action::Rename, key(KeyCode::F(2))),
        (Action::OpenTerminal, key(KeyCode::F(4))),
        (Action::Copy, key(KeyCode::F(5))),
        (Action::Mkdir, key(KeyCode::F(7))),
        (Action::Delete, key(KeyCode::F(8))),
        (Action::OpenConnections, key(KeyCode::F(9))),
        (Action::Back, key(KeyCode::Esc)),
        (Action::Refresh, key_with_modifiers(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        (Action::CancelTransfer, key_with_modifiers(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        (Action::ToggleHidden, key_with_modifiers(KeyCode::Char('h'), KeyModifiers::CONTROL)),
        (Action::CycleSort, key_with_modifiers(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        (Action::OpenTransfers, key_with_modifiers(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    ];
    for (action, event) in cases {
        assert_eq!(bindings.map_key(event), action);
    }
}

#[test]
fn an_unbound_key_maps_to_noop() {
    let bindings = KeyBindings::defaults();
    assert_eq!(bindings.map_key(key(KeyCode::Char('x'))), Action::Noop);
}

#[test]
fn parse_key_spec_parses_plain_function_keys() {
    assert_eq!(parse_key_spec("F10").unwrap(), KeySpec { code: KeyCode::F(10), modifiers: KeyModifiers::NONE });
}

#[test]
fn parse_key_spec_parses_ctrl_modifier_case_insensitively() {
    assert_eq!(
        parse_key_spec("Ctrl+R").unwrap(),
        KeySpec { code: KeyCode::Char('r'), modifiers: KeyModifiers::CONTROL }
    );
}

#[test]
fn parse_key_spec_parses_named_keys() {
    assert_eq!(parse_key_spec("tab").unwrap().code, KeyCode::Tab);
    assert_eq!(parse_key_spec("space").unwrap().code, KeyCode::Char(' '));
    assert_eq!(parse_key_spec("esc").unwrap().code, KeyCode::Esc);
}

#[test]
fn parse_key_spec_rejects_unrecognized_keys() {
    assert!(parse_key_spec("banana").is_err());
    assert!(parse_key_spec("f99").is_err());
}

#[test]
fn key_bindings_defaults_maps_f10_to_quit() {
    let bindings = KeyBindings::defaults();
    assert_eq!(bindings.map_key(key(KeyCode::F(10))), Action::Quit);
}

#[test]
fn from_overrides_applies_a_valid_override() {
    let mut overrides = HashMap::new();
    overrides.insert("quit".to_string(), "ctrl+q".to_string());

    let (bindings, warnings) = KeyBindings::from_overrides(&overrides);

    assert!(warnings.is_empty());
    assert_eq!(bindings.map_key(key_with_modifiers(KeyCode::Char('q'), KeyModifiers::CONTROL)), Action::Quit);
}

#[test]
fn from_overrides_warns_on_unknown_action_name() {
    let mut overrides = HashMap::new();
    overrides.insert("frobnicate".to_string(), "F10".to_string());

    let (_, warnings) = KeyBindings::from_overrides(&overrides);

    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("frobnicate"));
}

#[test]
fn from_overrides_warns_on_unparseable_key_string_and_keeps_the_default() {
    let mut overrides = HashMap::new();
    overrides.insert("quit".to_string(), "not-a-key".to_string());

    let (bindings, warnings) = KeyBindings::from_overrides(&overrides);

    assert_eq!(warnings.len(), 1);
    assert_eq!(bindings.map_key(key(KeyCode::F(10))), Action::Quit);
}

#[test]
fn from_overrides_warns_on_duplicate_binding() {
    let mut overrides = HashMap::new();
    overrides.insert("refresh".to_string(), "F10".to_string());

    let (_, warnings) = KeyBindings::from_overrides(&overrides);

    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("F10"));
}

#[test]
fn from_overrides_resolves_a_colliding_binding_deterministically() {
    let mut overrides = HashMap::new();
    overrides.insert("refresh".to_string(), "F10".to_string());

    let (bindings, warnings) = KeyBindings::from_overrides(&overrides);

    assert_eq!(warnings.len(), 1);
    assert_eq!(bindings.map_key(key(KeyCode::F(10))), Action::Refresh);
    assert_eq!(bindings.key_for(Action::Quit), None);
}

#[test]
fn defaults_never_bind_two_actions_to_the_same_key() {
    let bindings = KeyBindings::defaults();
    let mut seen: Vec<KeySpec> = Vec::new();
    for action in ALL_ACTIONS {
        let spec = bindings.key_for(*action).expect("every action in ALL_ACTIONS should have a default binding");
        assert!(!seen.contains(&spec), "action {:?} shares a default KeySpec {:?} with another action", action, spec);
        seen.push(spec);
    }
}

#[test]
fn action_name_and_from_name_round_trip() {
    for action in [Action::Quit, Action::ToggleHidden, Action::CycleSort, Action::Back, Action::OpenTransfers] {
        assert_eq!(Action::from_name(action.name()), Some(action));
    }
}

#[test]
fn from_name_rejects_noop() {
    assert_eq!(Action::from_name("noop"), None);
}

#[test]
fn f1_maps_to_help() {
    assert_eq!(map_key_via_defaults(KeyCode::F(1)), Action::Help);
}

#[test]
fn f6_is_bound_to_add_connection() {
    assert_eq!(map_key_via_defaults(KeyCode::F(6)), Action::AddConnection);
}

#[test]
fn ctrl_d_maps_to_bookmark_here() {
    assert_eq!(map_key_via_defaults_with_modifiers(KeyCode::Char('d'), KeyModifiers::CONTROL), Action::BookmarkHere);
}

#[test]
fn ctrl_b_maps_to_open_bookmarks() {
    assert_eq!(map_key_via_defaults_with_modifiers(KeyCode::Char('b'), KeyModifiers::CONTROL), Action::OpenBookmarks);
}

#[test]
fn ctrl_f_maps_to_open_search() {
    assert_eq!(map_key_via_defaults_with_modifiers(KeyCode::Char('f'), KeyModifiers::CONTROL), Action::OpenSearch);
}

#[test]
fn ctrl_n_maps_to_cycle_session() {
    assert_eq!(map_key_via_defaults_with_modifiers(KeyCode::Char('n'), KeyModifiers::CONTROL), Action::CycleSession);
}

#[test]
fn all_actions_excludes_noop() {
    assert!(!ALL_ACTIONS.contains(&Action::Noop));
}

#[test]
fn delete_key_is_bound_to_delete_connection() {
    assert_eq!(map_key_via_defaults(KeyCode::Delete), Action::DeleteConnection);
}

#[test]
fn ctrl_t_maps_to_open_transfers() {
    assert_eq!(map_key_via_defaults_with_modifiers(KeyCode::Char('t'), KeyModifiers::CONTROL), Action::OpenTransfers);
    assert_eq!(Action::OpenTransfers.name(), "open_transfers");
}
