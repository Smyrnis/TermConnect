use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use termconnect_core::Entry;

pub struct SearchView {
    pub pattern: String,
    pub cursor: usize,
    pub results: Vec<Entry>,
    pub selected: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOutcome {
    Pending,
    PatternChanged,
    Open,
    Cancel,
}

impl SearchView {
    pub fn new() -> Self {
        Self { pattern: String::new(), cursor: 0, results: Vec::new(), selected: 0, truncated: false }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SearchOutcome {
        match key.code {
            KeyCode::Esc => SearchOutcome::Cancel,
            KeyCode::Enter => SearchOutcome::Open,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                SearchOutcome::Pending
            }
            KeyCode::Down => {
                if self.selected + 1 < self.results.len() {
                    self.selected += 1;
                }
                SearchOutcome::Pending
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                SearchOutcome::Pending
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.pattern.chars().count());
                SearchOutcome::Pending
            }
            KeyCode::Home => {
                self.cursor = 0;
                SearchOutcome::Pending
            }
            KeyCode::End => {
                self.cursor = self.pattern.chars().count();
                SearchOutcome::Pending
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.remove_char_at(self.cursor - 1);
                self.cursor -= 1;
                SearchOutcome::PatternChanged
            }
            KeyCode::Delete if self.cursor < self.pattern.chars().count() => {
                self.remove_char_at(self.cursor);
                SearchOutcome::PatternChanged
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_char_at(self.cursor, c);
                self.cursor += 1;
                SearchOutcome::PatternChanged
            }
            _ => SearchOutcome::Pending,
        }
    }

    pub fn start(&mut self) {
        self.results.clear();
        self.selected = 0;
        self.truncated = false;
    }

    pub fn push_result(&mut self, entry: Entry) {
        self.results.push(entry);
    }

    pub fn finish(&mut self, truncated: bool) {
        self.truncated = truncated;
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.results.get(self.selected)
    }

    fn insert_char_at(&mut self, index: usize, ch: char) {
        let byte_index = self.byte_index_for(index);
        self.pattern.insert(byte_index, ch);
    }

    fn remove_char_at(&mut self, index: usize) {
        let byte_index = self.byte_index_for(index);
        self.pattern.remove(byte_index);
    }

    fn byte_index_for(&self, char_index: usize) -> usize {
        self.pattern.char_indices().nth(char_index).map(|(byte, _)| byte).unwrap_or(self.pattern.len())
    }
}

impl Default for SearchView {
    fn default() -> Self {
        Self::new()
    }
}

pub fn render_search(frame: &mut Frame, area: Rect, view: &SearchView) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

    let chars: Vec<char> = view.pattern.chars().collect();
    let mut spans: Vec<Span> = chars
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let style =
                if i == view.cursor { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
            Span::styled(ch.to_string(), style)
        })
        .collect();
    if view.cursor >= chars.len() {
        spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
    }
    let pattern_block = Block::default().title("Search (Esc to close, Ctrl+C to cancel)").borders(Borders::ALL);
    frame.render_widget(Paragraph::new(Line::from(spans)).block(pattern_block), rows[0]);

    let title = if view.truncated { "Results (truncated)" } else { "Results" };
    let items: Vec<ListItem> =
        view.results.iter().map(|entry| ListItem::new(entry.path.display().to_string())).collect();
    let mut state = ListState::default();
    if !view.results.is_empty() {
        state.select(Some(view.selected));
    }
    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, rows[1], &mut state);
}

#[cfg(test)]
mod tests;
