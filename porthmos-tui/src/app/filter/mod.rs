use super::*;
use crate::widgets::{filter_line::FilterLine, panel_view::PanelView};

impl App {
    fn active_panel_view(&self) -> Option<&PanelView> {
        match self.active_panel {
            ActivePanel::Local => Some(&self.local),
            ActivePanel::Remote => self.sessions.active().map(|session| &session.panel),
        }
    }

    fn active_panel_view_mut(&mut self) -> Option<&mut PanelView> {
        match self.active_panel {
            ActivePanel::Local => Some(&mut self.local),
            ActivePanel::Remote => self.sessions.active_mut().map(|session| &mut session.panel),
        }
    }

    fn active_filter_line(&self) -> Option<&dyn FilterLine> {
        match self.screen {
            Screen::Files => self.active_panel_view().map(|panel| panel as &dyn FilterLine),
            Screen::Connections => Some(&self.connections),
            _ => None,
        }
    }

    fn active_filter_line_mut(&mut self) -> Option<&mut dyn FilterLine> {
        match self.screen {
            Screen::Files => self.active_panel_view_mut().map(|panel| panel as &mut dyn FilterLine),
            Screen::Connections => Some(&mut self.connections),
            _ => None,
        }
    }

    pub(super) fn editing_filter(&self) -> bool {
        self.active_filter_line().is_some_and(FilterLine::editing_filter)
    }

    pub(super) fn start_filter(&mut self) {
        if let Some(line) = self.active_filter_line_mut() {
            line.start_filter();
        }
    }

    pub(super) fn clear_active_filter(&mut self) -> bool {
        match self.active_filter_line_mut() {
            Some(line) if line.filter().is_some() || line.editing_filter() => {
                line.clear_filter();
                true
            }
            _ => false,
        }
    }

    pub(super) fn apply_filter_key(&mut self, key: KeyEvent) {
        let Some(line) = self.active_filter_line_mut() else {
            return;
        };
        match key.code {
            KeyCode::Char(character) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                line.type_filter(character)
            }
            KeyCode::Backspace => line.erase_filter(),
            KeyCode::Enter => line.finish_filter(),
            KeyCode::Esc => line.clear_filter(),
            KeyCode::Up => line.move_cursor(-1),
            KeyCode::Down => line.move_cursor(1),
            _ => {
                line.finish_filter();
                let action = self.key_bindings.map_key(key);
                self.apply_action(action);
            }
        }
    }
}

#[cfg(test)]
mod tests;
