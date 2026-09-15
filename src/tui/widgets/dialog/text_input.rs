use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

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
        Self {
            title: title.into(),
            value,
            masked: false,
            cursor,
        }
    }

    pub fn new_masked(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            value: String::new(),
            masked: true,
            cursor: 0,
        }
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
        self.value
            .char_indices()
            .nth(char_index)
            .map(|(byte, _)| byte)
            .unwrap_or(self.value.len())
    }
}

pub fn render_text_input(frame: &mut Frame, area: Rect, dialog: &TextInputDialog) {
    let popup = centered_popup(area, 50, 4);

    let block = Block::default()
        .title(dialog.title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let displayed_value = if dialog.masked {
        "*".repeat(dialog.value.chars().count())
    } else {
        dialog.value.clone()
    };

    let chars: Vec<char> = displayed_value.chars().collect();
    let mut spans: Vec<Span> = chars
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let style = if i == dialog.cursor {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Span::styled(ch.to_string(), style)
        })
        .collect();
    if dialog.cursor >= chars.len() {
        spans.push(Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        ));
    }

    let paragraph = Paragraph::new(Line::from(spans)).block(block);

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

    #[test]
    fn new_masked_starts_empty_and_marks_masked() {
        let dialog = TextInputDialog::new_masked("Password");
        assert_eq!(dialog.value, "");
        assert!(dialog.masked);
    }

    #[test]
    fn masked_dialog_renders_asterisks_not_the_value() {
        let mut dialog = TextInputDialog::new_masked("Password");
        dialog.handle_key(key(KeyCode::Char('s')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('t')));

        let backend = ratatui::backend::TestBackend::new(60, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_text_input(frame, frame.area(), &dialog))
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("***"));
        assert!(!content.contains("set"));
    }

    #[test]
    fn left_and_right_move_the_cursor_without_changing_the_value() {
        let mut dialog = TextInputDialog::new("Name", "abc");
        assert_eq!(dialog.cursor, 3);

        dialog.handle_key(key(KeyCode::Left));
        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.cursor, 1);

        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.cursor, 2);
        assert_eq!(dialog.value, "abc");
    }

    #[test]
    fn home_and_end_jump_to_the_boundaries() {
        let mut dialog = TextInputDialog::new("Name", "abc");
        dialog.handle_key(key(KeyCode::Home));
        assert_eq!(dialog.cursor, 0);
        dialog.handle_key(key(KeyCode::End));
        assert_eq!(dialog.cursor, 3);
    }

    #[test]
    fn typing_inserts_at_the_cursor_not_only_at_the_end() {
        let mut dialog = TextInputDialog::new("Name", "ac");
        dialog.cursor = 1;
        dialog.handle_key(key(KeyCode::Char('b')));
        assert_eq!(dialog.value, "abc");
        assert_eq!(dialog.cursor, 2);
    }

    #[test]
    fn delete_removes_the_character_at_the_cursor() {
        let mut dialog = TextInputDialog::new("Name", "abc");
        dialog.cursor = 0;
        dialog.handle_key(key(KeyCode::Delete));
        assert_eq!(dialog.value, "bc");
        assert_eq!(dialog.cursor, 0);
    }

    #[test]
    fn backspace_at_the_start_does_nothing() {
        let mut dialog = TextInputDialog::new("Name", "abc");
        dialog.cursor = 0;
        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.value, "abc");
        assert_eq!(dialog.cursor, 0);
    }

    #[test]
    fn cursor_stays_within_bounds_on_an_empty_value() {
        let mut dialog = TextInputDialog::new("Name", "");
        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.cursor, 0);
        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.cursor, 0);
    }
}
