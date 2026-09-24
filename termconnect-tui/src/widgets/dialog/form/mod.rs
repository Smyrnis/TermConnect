use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct FormField {
    pub label: &'static str,
    pub value: String,
    pub cursor: usize,
    pub masked: bool,
}

impl FormField {
    pub fn new(label: &'static str, initial_value: impl Into<String>) -> Self {
        let value = initial_value.into();
        let cursor = value.chars().count();
        Self { label, value, cursor, masked: false }
    }

    pub fn new_masked(label: &'static str, initial_value: impl Into<String>) -> Self {
        let mut field = Self::new(label, initial_value);
        field.masked = true;
        field
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

pub struct FormDialog {
    pub title: String,
    pub fields: Vec<FormField>,
    pub focused: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormOutcome {
    Pending,
    Submitted(Vec<String>),
    Cancelled,
}

impl FormDialog {
    pub fn new(title: impl Into<String>, fields: Vec<FormField>) -> Self {
        Self { title: title.into(), fields, focused: 0, error: None }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FormOutcome {
        match key.code {
            KeyCode::Esc => FormOutcome::Cancelled,
            KeyCode::Enter => FormOutcome::Submitted(self.fields.iter().map(|f| f.value.clone()).collect()),
            KeyCode::Tab => {
                self.focused = (self.focused + 1) % self.fields.len();
                FormOutcome::Pending
            }
            KeyCode::BackTab => {
                self.focused = (self.focused + self.fields.len() - 1) % self.fields.len();
                FormOutcome::Pending
            }
            KeyCode::Left => {
                let field = &mut self.fields[self.focused];
                field.cursor = field.cursor.saturating_sub(1);
                FormOutcome::Pending
            }
            KeyCode::Right => {
                let field = &mut self.fields[self.focused];
                field.cursor = (field.cursor + 1).min(field.value.chars().count());
                FormOutcome::Pending
            }
            KeyCode::Home => {
                self.fields[self.focused].cursor = 0;
                FormOutcome::Pending
            }
            KeyCode::End => {
                let field = &mut self.fields[self.focused];
                field.cursor = field.value.chars().count();
                FormOutcome::Pending
            }
            KeyCode::Backspace => {
                let field = &mut self.fields[self.focused];
                if field.cursor > 0 {
                    field.remove_char_at(field.cursor - 1);
                    field.cursor -= 1;
                }
                FormOutcome::Pending
            }
            KeyCode::Delete => {
                let field = &mut self.fields[self.focused];
                if field.cursor < field.value.chars().count() {
                    field.remove_char_at(field.cursor);
                }
                FormOutcome::Pending
            }
            KeyCode::Char(c) => {
                let field = &mut self.fields[self.focused];
                field.insert_char_at(field.cursor, c);
                field.cursor += 1;
                FormOutcome::Pending
            }
            _ => FormOutcome::Pending,
        }
    }
}

pub fn render_form(frame: &mut Frame, area: Rect, dialog: &FormDialog) {
    let field_display_lines: Vec<String> = dialog
        .fields
        .iter()
        .map(|field| {
            let displayed = displayed_value(field);
            format!("{}: {}", field.label, displayed)
        })
        .collect();

    let mut width_lines: Vec<&str> = vec!["Protocol: SFTP"];
    width_lines.extend(field_display_lines.iter().map(String::as_str));
    if let Some(error) = &dialog.error {
        width_lines.push(error.as_str());
    }
    let width = super::content_width(&width_lines);
    let height = 1 + dialog.fields.len() as u16 + if dialog.error.is_some() { 1 } else { 0 } + 2;

    let popup = centered_popup(area, width, height);

    let block = Block::default()
        .title(dialog.title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let mut lines: Vec<Line> = vec![Line::from("Protocol: SFTP")];
    lines.extend(dialog.fields.iter().enumerate().map(|(i, field)| field_line(field, i == dialog.focused)));
    if let Some(error) = &dialog.error {
        lines.push(Line::styled(error.as_str(), Style::default().fg(Color::Red)));
    }

    let paragraph = Paragraph::new(lines).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn displayed_value(field: &FormField) -> String {
    if field.masked { "*".repeat(field.value.chars().count()) } else { field.value.clone() }
}

fn field_line(field: &FormField, focused: bool) -> Line<'static> {
    let displayed = displayed_value(field);
    let mut spans = vec![Span::raw(format!("{}: ", field.label))];

    if focused {
        let chars: Vec<char> = displayed.chars().collect();
        for (i, ch) in chars.iter().enumerate() {
            let style =
                if i == field.cursor { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
            spans.push(Span::styled(ch.to_string(), style));
        }
        if field.cursor >= chars.len() {
            spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
        }
    } else {
        spans.push(Span::raw(displayed));
    }

    Line::from(spans)
}

fn centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let [popup] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width.min(area.width))]).flex(Flex::Center).areas(popup);
    popup
}

#[cfg(test)]
mod tests;
