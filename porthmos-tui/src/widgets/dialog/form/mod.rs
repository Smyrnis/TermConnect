use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub const KEPT_SECRET: &str = "\u{0}kept";
const SAVED_SECRET_DISPLAY: &str = "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}  (saved)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Masked,
    SavedSecret,
    Choice { choices: Vec<(String, String)>, selected: usize },
}

pub struct FormField {
    pub key: &'static str,
    pub label: String,
    pub value: String,
    pub cursor: usize,
    pub kind: FieldKind,
}

impl FormField {
    pub fn text(key: &'static str, label: impl Into<String>, initial_value: impl Into<String>) -> Self {
        let value = initial_value.into();
        let cursor = value.chars().count();
        Self { key, label: label.into(), value, cursor, kind: FieldKind::Text }
    }

    pub fn masked(key: &'static str, label: impl Into<String>, initial_value: impl Into<String>) -> Self {
        Self { kind: FieldKind::Masked, ..Self::text(key, label, initial_value) }
    }

    pub fn saved_secret(key: &'static str, label: impl Into<String>) -> Self {
        Self { kind: FieldKind::SavedSecret, ..Self::text(key, label, "") }
    }

    pub fn choice(
        key: &'static str, label: impl Into<String>, choices: Vec<(String, String)>, selected_value: &str,
    ) -> Self {
        let selected = choices.iter().position(|(value, _)| value == selected_value).unwrap_or(0);
        Self {
            key,
            label: label.into(),
            value: String::new(),
            cursor: 0,
            kind: FieldKind::Choice { choices, selected },
        }
    }

    pub fn submitted_value(&self) -> String {
        match &self.kind {
            FieldKind::Choice { choices, selected } => {
                choices.get(*selected).map(|(value, _)| value.clone()).unwrap_or_default()
            }
            FieldKind::SavedSecret => KEPT_SECRET.to_string(),
            FieldKind::Text | FieldKind::Masked => self.value.clone(),
        }
    }

    fn is_choice(&self) -> bool {
        matches!(self.kind, FieldKind::Choice { .. })
    }

    fn shift_choice(&mut self, forward: bool) -> bool {
        let FieldKind::Choice { choices, selected } = &mut self.kind else {
            return false;
        };
        if choices.len() < 2 {
            return false;
        }
        *selected =
            if forward { (*selected + 1) % choices.len() } else { (*selected + choices.len() - 1) % choices.len() };
        true
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
    pub hint: Option<String>,
    pub saved_secrets: Option<(String, Vec<String>)>,
    pub remembered: BTreeMap<String, String>,
    pub prefilled: BTreeMap<&'static str, String>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum FormOutcome {
    Pending,
    Submitted(Vec<(&'static str, String)>),
    ChoiceChanged { key: &'static str },
    Cancelled,
}

pub(super) fn submitted_keys(values: &[(&'static str, String)]) -> Vec<&'static str> {
    values.iter().map(|(key, _)| *key).collect()
}

impl std::fmt::Debug for FormOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormOutcome::Pending => write!(f, "Pending"),
            FormOutcome::Submitted(values) => f.debug_tuple("Submitted").field(&submitted_keys(values)).finish(),
            FormOutcome::ChoiceChanged { key } => f.debug_struct("ChoiceChanged").field("key", key).finish(),
            FormOutcome::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl FormDialog {
    pub fn new(title: impl Into<String>, fields: Vec<FormField>) -> Self {
        Self {
            title: title.into(),
            fields,
            focused: 0,
            error: None,
            hint: None,
            saved_secrets: None,
            remembered: BTreeMap::new(),
            prefilled: BTreeMap::new(),
        }
    }

    pub fn value(&self, key: &str) -> Option<String> {
        self.fields.iter().find(|field| field.key == key).map(FormField::submitted_value)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FormOutcome {
        let on_choice = self.fields[self.focused].is_choice();
        let field = &mut self.fields[self.focused];
        if field.kind == FieldKind::SavedSecret {
            match key.code {
                KeyCode::Char(c) => {
                    field.kind = FieldKind::Masked;
                    field.value = c.to_string();
                    field.cursor = 1;
                    return FormOutcome::Pending;
                }
                KeyCode::Backspace | KeyCode::Delete => {
                    field.kind = FieldKind::Masked;
                    field.value.clear();
                    field.cursor = 0;
                    return FormOutcome::Pending;
                }
                _ => {}
            }
        }
        match key.code {
            KeyCode::Esc => FormOutcome::Cancelled,
            KeyCode::Enter => {
                FormOutcome::Submitted(self.fields.iter().map(|field| (field.key, field.submitted_value())).collect())
            }
            KeyCode::Left | KeyCode::Right if on_choice => {
                let field = &mut self.fields[self.focused];
                if field.shift_choice(key.code == KeyCode::Right) {
                    FormOutcome::ChoiceChanged { key: field.key }
                } else {
                    FormOutcome::Pending
                }
            }
            KeyCode::Home | KeyCode::End | KeyCode::Backspace | KeyCode::Delete | KeyCode::Char(_) if on_choice => {
                FormOutcome::Pending
            }
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

    let mut width_lines: Vec<&str> = field_display_lines.iter().map(String::as_str).collect();
    width_lines.extend(dialog.hint.as_deref());
    if let Some(error) = &dialog.error {
        width_lines.push(error.as_str());
    }
    let width = super::content_width(&width_lines);
    let height =
        dialog.fields.len() as u16 + u16::from(dialog.hint.is_some()) + if dialog.error.is_some() { 1 } else { 0 } + 2;

    let popup = centered_popup(area, width, height);

    let block = Block::default()
        .title(dialog.title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let mut lines: Vec<Line> =
        dialog.fields.iter().enumerate().map(|(i, field)| field_line(field, i == dialog.focused)).collect();
    if let Some(hint) = &dialog.hint {
        lines.push(Line::styled(hint.as_str(), Style::default().add_modifier(Modifier::DIM)));
    }
    if let Some(error) = &dialog.error {
        lines.push(Line::styled(error.as_str(), Style::default().fg(Color::Red)));
    }

    let paragraph = Paragraph::new(lines).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

fn displayed_value(field: &FormField) -> String {
    match &field.kind {
        FieldKind::Text => field.value.clone(),
        FieldKind::Masked => "*".repeat(field.value.chars().count()),
        FieldKind::SavedSecret => SAVED_SECRET_DISPLAY.to_string(),
        FieldKind::Choice { choices, selected } => {
            format!("\u{25c0} {} \u{25b6}", choices.get(*selected).map(|(_, label)| label.as_str()).unwrap_or(""))
        }
    }
}

fn field_line(field: &FormField, focused: bool) -> Line<'static> {
    let displayed = displayed_value(field);
    let mut spans = vec![Span::raw(format!("{}: ", field.label))];

    if focused && field.is_choice() {
        spans.push(Span::styled(displayed, Style::default().add_modifier(Modifier::REVERSED)));
    } else if focused {
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
