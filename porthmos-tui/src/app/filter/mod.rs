use super::*;
use crate::widgets::panel_view::PanelView;

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

    pub(super) fn editing_filter(&self) -> bool {
        self.screen == Screen::Files && self.active_panel_view().is_some_and(PanelView::editing_filter)
    }

    pub(super) fn start_filter(&mut self) {
        if self.screen != Screen::Files {
            return;
        }
        if let Some(panel) = self.active_panel_view_mut() {
            panel.start_filter();
        }
    }

    pub(super) fn clear_active_filter(&mut self) -> bool {
        if self.screen != Screen::Files {
            return false;
        }
        match self.active_panel_view_mut() {
            Some(panel) if panel.filter().is_some() || panel.editing_filter() => {
                panel.clear_filter();
                true
            }
            _ => false,
        }
    }

    pub(super) fn apply_filter_key(&mut self, key: KeyEvent) {
        let Some(panel) = self.active_panel_view_mut() else {
            return;
        };
        match key.code {
            KeyCode::Char(character) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                panel.type_filter(character)
            }
            KeyCode::Backspace => panel.erase_filter(),
            KeyCode::Enter => panel.finish_filter(),
            KeyCode::Esc => panel.clear_filter(),
            KeyCode::Up => panel.move_cursor(-1),
            KeyCode::Down => panel.move_cursor(1),
            _ => {
                panel.finish_filter();
                let action = self.key_bindings.map_key(key);
                self.apply_action(action);
            }
        }
    }
}

#[cfg(test)]
mod tests;
