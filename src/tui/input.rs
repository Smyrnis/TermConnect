use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Quit,
    SwitchPanel,
    Up,
    Down,
    Open,
    ToggleSelect,
    Rename,
    Mkdir,
    Delete,
    Refresh,
    Copy,
    CancelTransfer,
    OpenConnections,
    OpenTerminal,
    ToggleHidden,
    CycleSort,
    Back,
    Noop,
}

impl Action {
    pub fn name(&self) -> &'static str {
        match self {
            Action::Quit => "quit",
            Action::SwitchPanel => "switch_panel",
            Action::Up => "up",
            Action::Down => "down",
            Action::Open => "open",
            Action::ToggleSelect => "toggle_select",
            Action::Rename => "rename",
            Action::Mkdir => "mkdir",
            Action::Delete => "delete",
            Action::Refresh => "refresh",
            Action::Copy => "copy",
            Action::CancelTransfer => "cancel_transfer",
            Action::OpenConnections => "open_connections",
            Action::OpenTerminal => "open_terminal",
            Action::ToggleHidden => "toggle_hidden",
            Action::CycleSort => "cycle_sort",
            Action::Back => "back",
            Action::Noop => "noop",
        }
    }

    /// The inverse of `name`. Returns `None` for `"noop"` too — it's the
    /// event loop's internal "nothing matched" sentinel, not a bindable
    /// action a user should be able to target from `config.toml`.
    pub fn from_name(name: &str) -> Option<Action> {
        Some(match name {
            "quit" => Action::Quit,
            "switch_panel" => Action::SwitchPanel,
            "up" => Action::Up,
            "down" => Action::Down,
            "open" => Action::Open,
            "toggle_select" => Action::ToggleSelect,
            "rename" => Action::Rename,
            "mkdir" => Action::Mkdir,
            "delete" => Action::Delete,
            "refresh" => Action::Refresh,
            "copy" => Action::Copy,
            "cancel_transfer" => Action::CancelTransfer,
            "open_connections" => Action::OpenConnections,
            "open_terminal" => Action::OpenTerminal,
            "toggle_hidden" => Action::ToggleHidden,
            "cycle_sort" => Action::CycleSort,
            "back" => Action::Back,
            _ => return None,
        })
    }
}

pub fn map_key(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::F(10) => Action::Quit,
        KeyCode::Tab => Action::SwitchPanel,
        KeyCode::Up => Action::Up,
        KeyCode::Down => Action::Down,
        KeyCode::Enter => Action::Open,
        KeyCode::Char(' ') => Action::ToggleSelect,
        KeyCode::F(2) => Action::Rename,
        KeyCode::F(4) => Action::OpenTerminal,
        KeyCode::F(5) => Action::Copy,
        KeyCode::F(7) => Action::Mkdir,
        KeyCode::F(8) => Action::Delete,
        KeyCode::F(9) => Action::OpenConnections,
        KeyCode::Esc => Action::Back,
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Refresh,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Action::CancelTransfer
        }
        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::ToggleHidden,
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::CycleSort,
        _ => Action::Noop,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKeyError(pub String);

impl std::fmt::Display for ParseKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseKeyError {}

/// Parses a binding string like `"ctrl+h"` or `"F10"`, case-insensitively.
/// Accepts any combination of `ctrl+`/`alt+`/`shift+` prefixes followed by
/// a function key (`f1`-`f12`), a named key, or a single character.
pub fn parse_key_spec(s: &str) -> Result<KeySpec, ParseKeyError> {
    let mut modifiers = KeyModifiers::NONE;
    let mut remainder = s.to_lowercase();

    loop {
        if let Some(rest) = remainder.strip_prefix("ctrl+") {
            modifiers |= KeyModifiers::CONTROL;
            remainder = rest.to_string();
        } else if let Some(rest) = remainder.strip_prefix("alt+") {
            modifiers |= KeyModifiers::ALT;
            remainder = rest.to_string();
        } else if let Some(rest) = remainder.strip_prefix("shift+") {
            modifiers |= KeyModifiers::SHIFT;
            remainder = rest.to_string();
        } else {
            break;
        }
    }

    let code = parse_key_code(&remainder)?;
    Ok(KeySpec { code, modifiers })
}

fn parse_key_code(s: &str) -> Result<KeyCode, ParseKeyError> {
    if let Some(digits) = s.strip_prefix('f')
        && let Ok(num) = digits.parse::<u8>()
        && (1..=12).contains(&num)
    {
        return Ok(KeyCode::F(num));
    }

    match s {
        "tab" => Ok(KeyCode::Tab),
        "enter" => Ok(KeyCode::Enter),
        "esc" | "escape" => Ok(KeyCode::Esc),
        "space" => Ok(KeyCode::Char(' ')),
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        "home" => Ok(KeyCode::Home),
        "end" => Ok(KeyCode::End),
        "backspace" => Ok(KeyCode::Backspace),
        "delete" | "del" => Ok(KeyCode::Delete),
        _ => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Ok(KeyCode::Char(ch)),
                _ => Err(ParseKeyError(format!("unrecognized key \"{s}\""))),
            }
        }
    }
}

pub struct KeyBindings(HashMap<Action, KeySpec>);

impl KeyBindings {
    pub fn defaults() -> Self {
        let mut map = HashMap::new();
        let mut bind = |action: Action, code: KeyCode, modifiers: KeyModifiers| {
            map.insert(action, KeySpec { code, modifiers });
        };

        bind(Action::Quit, KeyCode::F(10), KeyModifiers::NONE);
        bind(Action::SwitchPanel, KeyCode::Tab, KeyModifiers::NONE);
        bind(Action::Up, KeyCode::Up, KeyModifiers::NONE);
        bind(Action::Down, KeyCode::Down, KeyModifiers::NONE);
        bind(Action::Open, KeyCode::Enter, KeyModifiers::NONE);
        bind(Action::ToggleSelect, KeyCode::Char(' '), KeyModifiers::NONE);
        bind(Action::Rename, KeyCode::F(2), KeyModifiers::NONE);
        bind(Action::OpenTerminal, KeyCode::F(4), KeyModifiers::NONE);
        bind(Action::Copy, KeyCode::F(5), KeyModifiers::NONE);
        bind(Action::Mkdir, KeyCode::F(7), KeyModifiers::NONE);
        bind(Action::Delete, KeyCode::F(8), KeyModifiers::NONE);
        bind(Action::OpenConnections, KeyCode::F(9), KeyModifiers::NONE);
        bind(Action::Back, KeyCode::Esc, KeyModifiers::NONE);
        bind(Action::Refresh, KeyCode::Char('r'), KeyModifiers::CONTROL);
        bind(
            Action::CancelTransfer,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        );
        bind(
            Action::ToggleHidden,
            KeyCode::Char('h'),
            KeyModifiers::CONTROL,
        );
        bind(Action::CycleSort, KeyCode::Char('s'), KeyModifiers::CONTROL);

        Self(map)
    }

    /// Builds bindings from the defaults, replacing one entry per valid
    /// `[keys]` line. Never fails: an unknown action name, an unparseable
    /// key string, or a key that collides with another action's binding
    /// each produce one warning string and otherwise leave the default (or
    /// prior override) in place, so `App` can surface them as startup
    /// notifications without blocking.
    pub fn from_overrides(overrides: &HashMap<String, String>) -> (Self, Vec<String>) {
        let mut bindings = Self::defaults();
        let mut warnings = Vec::new();

        for (action_name, key_str) in overrides {
            let Some(action) = Action::from_name(action_name) else {
                warnings.push(format!("unknown key binding action \"{action_name}\""));
                continue;
            };

            let spec = match parse_key_spec(key_str) {
                Ok(spec) => spec,
                Err(err) => {
                    warnings.push(format!(
                        "invalid key \"{key_str}\" for \"{action_name}\": {err}"
                    ));
                    continue;
                }
            };

            if let Some((conflicting, _)) = bindings
                .0
                .iter()
                .find(|(a, s)| **a != action && **s == spec)
            {
                warnings.push(format!(
                    "key \"{key_str}\" is already bound to \"{}\"; \"{action_name}\" now overrides it",
                    conflicting.name()
                ));
            }

            bindings.0.insert(action, spec);
        }

        (bindings, warnings)
    }

    pub fn map_key(&self, key: KeyEvent) -> Action {
        let spec = KeySpec {
            code: key.code,
            modifiers: key.modifiers,
        };
        self.0
            .iter()
            .find(|(_, bound)| **bound == spec)
            .map(|(action, _)| *action)
            .unwrap_or(Action::Noop)
    }

    /// The key currently bound to `action`, for the help overlay (Task 16)
    /// and the status-bar hint text.
    pub fn key_for(&self, action: Action) -> Option<KeySpec> {
        self.0.get(&action).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> KeyEvent {
        key_with_modifiers(code, KeyModifiers::NONE)
    }

    fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn f10_maps_to_quit() {
        assert_eq!(map_key(key(KeyCode::F(10))), Action::Quit);
    }

    #[test]
    fn tab_maps_to_switch_panel() {
        assert_eq!(map_key(key(KeyCode::Tab)), Action::SwitchPanel);
    }

    #[test]
    fn arrow_keys_map_to_navigation() {
        assert_eq!(map_key(key(KeyCode::Up)), Action::Up);
        assert_eq!(map_key(key(KeyCode::Down)), Action::Down);
    }

    #[test]
    fn enter_maps_to_open() {
        assert_eq!(map_key(key(KeyCode::Enter)), Action::Open);
    }

    #[test]
    fn space_maps_to_toggle_select() {
        assert_eq!(map_key(key(KeyCode::Char(' '))), Action::ToggleSelect);
    }

    #[test]
    fn function_keys_map_to_file_operations() {
        assert_eq!(map_key(key(KeyCode::F(2))), Action::Rename);
        assert_eq!(map_key(key(KeyCode::F(7))), Action::Mkdir);
        assert_eq!(map_key(key(KeyCode::F(8))), Action::Delete);
    }

    #[test]
    fn f5_maps_to_copy() {
        assert_eq!(map_key(key(KeyCode::F(5))), Action::Copy);
    }

    #[test]
    fn f4_maps_to_open_terminal() {
        assert_eq!(map_key(key(KeyCode::F(4))), Action::OpenTerminal);
    }

    #[test]
    fn ctrl_c_maps_to_cancel_transfer() {
        assert_eq!(
            map_key(key_with_modifiers(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL
            )),
            Action::CancelTransfer
        );
    }

    #[test]
    fn f9_maps_to_open_connections() {
        assert_eq!(map_key(key(KeyCode::F(9))), Action::OpenConnections);
    }

    #[test]
    fn esc_maps_to_back() {
        assert_eq!(map_key(key(KeyCode::Esc)), Action::Back);
    }

    #[test]
    fn ctrl_r_maps_to_refresh() {
        assert_eq!(
            map_key(key_with_modifiers(
                KeyCode::Char('r'),
                KeyModifiers::CONTROL
            )),
            Action::Refresh
        );
    }

    #[test]
    fn plain_r_without_control_maps_to_noop() {
        assert_eq!(map_key(key(KeyCode::Char('r'))), Action::Noop);
    }

    #[test]
    fn other_keys_map_to_noop() {
        assert_eq!(map_key(key(KeyCode::Char('x'))), Action::Noop);
    }

    #[test]
    fn ctrl_h_maps_to_toggle_hidden() {
        assert_eq!(
            map_key(key_with_modifiers(
                KeyCode::Char('h'),
                KeyModifiers::CONTROL
            )),
            Action::ToggleHidden
        );
    }

    #[test]
    fn ctrl_s_maps_to_cycle_sort() {
        assert_eq!(
            map_key(key_with_modifiers(
                KeyCode::Char('s'),
                KeyModifiers::CONTROL
            )),
            Action::CycleSort
        );
    }

    #[test]
    fn parse_key_spec_parses_plain_function_keys() {
        assert_eq!(
            parse_key_spec("F10").unwrap(),
            KeySpec {
                code: KeyCode::F(10),
                modifiers: KeyModifiers::NONE
            }
        );
    }

    #[test]
    fn parse_key_spec_parses_ctrl_modifier_case_insensitively() {
        assert_eq!(
            parse_key_spec("Ctrl+R").unwrap(),
            KeySpec {
                code: KeyCode::Char('r'),
                modifiers: KeyModifiers::CONTROL
            }
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
        assert_eq!(
            bindings.map_key(key_with_modifiers(
                KeyCode::Char('q'),
                KeyModifiers::CONTROL
            )),
            Action::Quit
        );
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
        overrides.insert("refresh".to_string(), "F10".to_string()); // F10 is Quit's default

        let (_, warnings) = KeyBindings::from_overrides(&overrides);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("F10"));
    }

    #[test]
    fn action_name_and_from_name_round_trip() {
        for action in [
            Action::Quit,
            Action::ToggleHidden,
            Action::CycleSort,
            Action::Back,
        ] {
            assert_eq!(Action::from_name(action.name()), Some(action));
        }
    }

    #[test]
    fn from_name_rejects_noop() {
        assert_eq!(Action::from_name("noop"), None);
    }
}
