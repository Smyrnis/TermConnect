use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct TextInputDialog {
    pub title: String,
    pub value: String,
    pub masked: bool,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputOutcome {
    Pending,
    Submitted(String),
    Cancelled,
}

impl TextInputDialog {
    pub fn new(title: impl Into<String>, initial_value: impl Into<String>) -> Self {
        let value = initial_value.into();
        let cursor = value.chars().count();
        Self { title: title.into(), value, masked: false, cursor }
    }

    pub fn new_masked(title: impl Into<String>) -> Self {
        Self { title: title.into(), value: String::new(), masked: true, cursor: 0 }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TextInputOutcome {
        match key.code {
            KeyCode::Enter => TextInputOutcome::Submitted(self.value.clone()),
            KeyCode::Esc => TextInputOutcome::Cancelled,
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                TextInputOutcome::Pending
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.value.chars().count());
                TextInputOutcome::Pending
            }
            KeyCode::Home => {
                self.cursor = 0;
                TextInputOutcome::Pending
            }
            KeyCode::End => {
                self.cursor = self.value.chars().count();
                TextInputOutcome::Pending
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.remove_char_at(self.cursor - 1);
                    self.cursor -= 1;
                }
                TextInputOutcome::Pending
            }
            KeyCode::Delete => {
                if self.cursor < self.value.chars().count() {
                    self.remove_char_at(self.cursor);
                }
                TextInputOutcome::Pending
            }
            KeyCode::Char(c) => {
                self.insert_char_at(self.cursor, c);
                self.cursor += 1;
                TextInputOutcome::Pending
            }
            _ => TextInputOutcome::Pending,
        }
    }

    fn insert_char_at(&mut self, index: usize, ch: char) {
        let byte_index = self.byte_index_for(index);
        self.value.insert(byte_index, ch);
    }

    fn remove_char_at(&mut self, index: usize) {
        let byte_index = self.byte_index_for(index);
        self.value.remove(byte_index);
    }

    fn byte_index_for(&self, char_index: usize) -> usize {
        self.value.char_indices().nth(char_index).map(|(byte, _)| byte).unwrap_or(self.value.len())
    }
}

pub fn render_text_input(frame: &mut Frame, area: Rect, dialog: &TextInputDialog) {
    let displayed_value = if dialog.masked { "*".repeat(dialog.value.chars().count()) } else { dialog.value.clone() };

    let popup = centered_popup(area, super::content_width(&[dialog.title.as_str(), displayed_value.as_str()]), 4);

    let block = Block::default()
        .title(dialog.title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let chars: Vec<char> = displayed_value.chars().collect();
    let mut spans: Vec<Span> = chars
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let style =
                if i == dialog.cursor { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
            Span::styled(ch.to_string(), style)
        })
        .collect();
    if dialog.cursor >= chars.len() {
        spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
    }

    let paragraph = Paragraph::new(Line::from(spans)).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let [popup] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width.min(area.width))]).flex(Flex::Center).areas(popup);
    popup
}

#[cfg(test)]
mod tests;
