use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Back,
    Noop,
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
        KeyCode::F(5) => Action::Copy,
        KeyCode::F(7) => Action::Mkdir,
        KeyCode::F(8) => Action::Delete,
        KeyCode::F(9) => Action::OpenConnections,
        KeyCode::Esc => Action::Back,
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Refresh,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Action::CancelTransfer
        }
        _ => Action::Noop,
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
}
