use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

pub struct ListDialog {
    pub title: String,
    pub items: Vec<String>,
    pub cursor: usize,
    pub removable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListOutcome {
    Pending,
    Selected(usize),
    Removed(usize),
    Cancelled,
}

impl ListDialog {
    pub fn new(title: impl Into<String>, items: Vec<String>) -> Self {
        Self { title: title.into(), items, cursor: 0, removable: false }
    }

    pub fn removable(mut self, removable: bool) -> Self {
        self.removable = removable;
        self
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ListOutcome {
        match key.code {
            KeyCode::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                ListOutcome::Pending
            }
            KeyCode::Down => {
                if self.cursor + 1 < self.items.len() {
                    self.cursor += 1;
                }
                ListOutcome::Pending
            }
            KeyCode::Enter if !self.items.is_empty() => ListOutcome::Selected(self.cursor),
            KeyCode::F(8) if self.removable && !self.items.is_empty() => ListOutcome::Removed(self.cursor),
            KeyCode::Esc => ListOutcome::Cancelled,
            _ => ListOutcome::Pending,
        }
    }
}

pub fn render_list(frame: &mut Frame, area: Rect, dialog: &ListDialog) {
    let mut width_lines: Vec<&str> = dialog.items.iter().map(String::as_str).collect();
    width_lines.push(dialog.title.as_str());
    let width = super::content_width(&width_lines);
    let height = (dialog.items.len() as u16 + 2).clamp(3, area.height.saturating_sub(2).max(3));
    let popup = centered_popup(area, width, height);

    let block = Block::default().title(dialog.title.as_str()).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));

    let items: Vec<ListItem> = if dialog.items.is_empty() { vec![ListItem::new("(empty)")] } else { dialog.items.iter().map(|item| ListItem::new(item.as_str())).collect() };

    let mut state = ListState::default();
    if !dialog.items.is_empty() {
        state.select(Some(dialog.cursor));
    }

    let list = List::new(items).block(block).highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_widget(Clear, popup);
    frame.render_stateful_widget(list, popup, &mut state);
}

fn centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let [popup] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width.min(area.width))]).flex(Flex::Center).areas(popup);
    popup
}

#[cfg(test)]
#[path = "../../../tests/tui/dialog/list_test.rs"]
mod tests;
