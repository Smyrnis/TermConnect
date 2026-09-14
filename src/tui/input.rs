use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    SwitchPanel,
    Noop,
}

pub fn map_key(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::F(10) => Action::Quit,
        KeyCode::Tab => Action::SwitchPanel,
        _ => Action::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
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
    fn other_keys_map_to_noop() {
        assert_eq!(map_key(key(KeyCode::Char('x'))), Action::Noop);
    }
}
