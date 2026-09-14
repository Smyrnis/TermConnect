use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub struct TextInputDialog {
    pub title: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputOutcome {
    Pending,
    Submitted(String),
    Cancelled,
}

impl TextInputDialog {
    pub fn new(title: impl Into<String>, initial_value: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            value: initial_value.into(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TextInputOutcome {
        match key.code {
            KeyCode::Enter => TextInputOutcome::Submitted(self.value.clone()),
            KeyCode::Esc => TextInputOutcome::Cancelled,
            KeyCode::Backspace => {
                self.value.pop();
                TextInputOutcome::Pending
            }
            KeyCode::Char(c) => {
                self.value.push(c);
                TextInputOutcome::Pending
            }
            _ => TextInputOutcome::Pending,
        }
    }
}

pub fn render_text_input(frame: &mut Frame, area: Rect, dialog: &TextInputDialog) {
    let popup = centered_popup(area, 50, 4);

    let block = Block::default()
        .title(dialog.title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let text = Text::from(vec![Line::from(format!("{}\u{2588}", dialog.value))]);

    let paragraph = Paragraph::new(text).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let [popup] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(popup);
    popup
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
    fn typing_characters_appends_to_value() {
        let mut dialog = TextInputDialog::new("New name", "");
        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        assert_eq!(dialog.value, "ab");
    }

    #[test]
    fn backspace_removes_the_last_character() {
        let mut dialog = TextInputDialog::new("New name", "ab");
        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.value, "a");
    }

    #[test]
    fn enter_submits_the_current_value() {
        let mut dialog = TextInputDialog::new("New name", "final");
        let outcome = dialog.handle_key(key(KeyCode::Enter));
        assert_eq!(outcome, TextInputOutcome::Submitted("final".to_string()));
    }

    #[test]
    fn esc_cancels() {
        let mut dialog = TextInputDialog::new("New name", "final");
        let outcome = dialog.handle_key(key(KeyCode::Esc));
        assert_eq!(outcome, TextInputOutcome::Cancelled);
    }
}
