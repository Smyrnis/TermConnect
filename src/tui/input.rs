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
    AddConnection,
    DeleteConnection,
    OpenTerminal,
    ToggleHidden,
    CycleSort,
    BookmarkHere,
    OpenBookmarks,
    OpenSearch,
    CycleSession,
    Help,
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
            Action::AddConnection => "add_connection",
            Action::DeleteConnection => "delete_connection",
            Action::OpenTerminal => "open_terminal",
            Action::ToggleHidden => "toggle_hidden",
            Action::CycleSort => "cycle_sort",
            Action::BookmarkHere => "bookmark_here",
            Action::OpenBookmarks => "open_bookmarks",
            Action::OpenSearch => "open_search",
            Action::CycleSession => "cycle_session",
            Action::Help => "help",
            Action::Back => "back",
            Action::Noop => "noop",
        }
    }

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
            "add_connection" => Action::AddConnection,
            "delete_connection" => Action::DeleteConnection,
            "open_terminal" => Action::OpenTerminal,
            "toggle_hidden" => Action::ToggleHidden,
            "cycle_sort" => Action::CycleSort,
            "bookmark_here" => Action::BookmarkHere,
            "open_bookmarks" => Action::OpenBookmarks,
            "open_search" => Action::OpenSearch,
            "cycle_session" => Action::CycleSession,
            "help" => Action::Help,
            "back" => Action::Back,
            _ => return None,
        })
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

pub fn format_key_spec(spec: KeySpec) -> String {
    let mut parts = Vec::new();
    if spec.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if spec.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }
    if spec.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".to_string());
    }

    let key_name = match spec.code {
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Char(c) => c.to_uppercase().to_string(),
        other => format!("{other:?}"),
    };
    parts.push(key_name);

    parts.join("+")
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
        bind(Action::AddConnection, KeyCode::F(6), KeyModifiers::NONE);
        bind(Action::DeleteConnection, KeyCode::Delete, KeyModifiers::NONE);
        bind(Action::Back, KeyCode::Esc, KeyModifiers::NONE);
        bind(Action::Refresh, KeyCode::Char('r'), KeyModifiers::CONTROL);
        bind(Action::CancelTransfer, KeyCode::Char('c'), KeyModifiers::CONTROL);
        bind(Action::ToggleHidden, KeyCode::Char('h'), KeyModifiers::CONTROL);
        bind(Action::CycleSort, KeyCode::Char('s'), KeyModifiers::CONTROL);
        bind(Action::BookmarkHere, KeyCode::Char('d'), KeyModifiers::CONTROL);
        bind(Action::OpenBookmarks, KeyCode::Char('b'), KeyModifiers::CONTROL);
        bind(Action::OpenSearch, KeyCode::Char('f'), KeyModifiers::CONTROL);
        bind(Action::CycleSession, KeyCode::Char('n'), KeyModifiers::CONTROL);
        bind(Action::Help, KeyCode::F(1), KeyModifiers::NONE);

        Self(map)
    }

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
                    warnings.push(format!("invalid key \"{key_str}\" for \"{action_name}\": {err}"));
                    continue;
                }
            };

            if let Some((conflicting, _)) = bindings.0.iter().find(|(a, s)| **a != action && **s == spec) {
                warnings.push(format!(
                    "key \"{key_str}\" is already bound to \"{}\"; \"{action_name}\" now overrides it",
                    conflicting.name()
                ));
            }

            bindings.0.retain(|existing_action, existing_spec| *existing_action == action || *existing_spec != spec);
            bindings.0.insert(action, spec);
        }

        (bindings, warnings)
    }

    pub fn map_key(&self, key: KeyEvent) -> Action {
        let spec = KeySpec { code: key.code, modifiers: key.modifiers };
        self.0.iter().find(|(_, bound)| **bound == spec).map(|(action, _)| *action).unwrap_or(Action::Noop)
    }

    pub fn key_for(&self, action: Action) -> Option<KeySpec> {
        self.0.get(&action).copied()
    }
}

pub const ALL_ACTIONS: &[Action] = &[
    Action::Quit,
    Action::SwitchPanel,
    Action::Up,
    Action::Down,
    Action::Open,
    Action::ToggleSelect,
    Action::Rename,
    Action::Mkdir,
    Action::Delete,
    Action::Refresh,
    Action::Copy,
    Action::CancelTransfer,
    Action::OpenConnections,
    Action::AddConnection,
    Action::DeleteConnection,
    Action::OpenTerminal,
    Action::ToggleHidden,
    Action::CycleSort,
    Action::BookmarkHere,
    Action::OpenBookmarks,
    Action::OpenSearch,
    Action::CycleSession,
    Action::Help,
    Action::Back,
];

#[cfg(test)]
#[path = "../../tests/tui/input_test.rs"]
mod tests;
