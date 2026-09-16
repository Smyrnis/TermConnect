use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::filesystem::Entry;

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
        Self {
            pattern: String::new(),
            cursor: 0,
            results: Vec::new(),
            selected: 0,
            truncated: false,
        }
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

    /// Resets for a new search — called right before spawning one.
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
        self.pattern
            .char_indices()
            .nth(char_index)
            .map(|(byte, _)| byte)
            .unwrap_or(self.pattern.len())
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
            let style = if i == view.cursor {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Span::styled(ch.to_string(), style)
        })
        .collect();
    if view.cursor >= chars.len() {
        spans.push(Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        ));
    }
    let pattern_block = Block::default()
        .title("Search (Esc to close, Ctrl+C to cancel)")
        .borders(Borders::ALL);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(pattern_block),
        rows[0],
    );

    let title = if view.truncated {
        "Results (truncated)"
    } else {
        "Results"
    };
    let items: Vec<ListItem> = view
        .results
        .iter()
        .map(|entry| ListItem::new(entry.path.display().to_string()))
        .collect();
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
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
    use std::path::PathBuf;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            path: PathBuf::from(format!("/{name}")),
            is_dir: false,
            size: 0,
            permissions: None,
        }
    }

    #[test]
    fn typing_inserts_at_the_cursor_and_reports_pattern_changed() {
        let mut view = SearchView::new();
        assert_eq!(
            view.handle_key(key(KeyCode::Char('a'))),
            SearchOutcome::PatternChanged
        );
        assert_eq!(view.pattern, "a");
        assert_eq!(view.cursor, 1);
    }

    #[test]
    fn ctrl_modified_characters_are_not_inserted() {
        let mut view = SearchView::new();
        let event = KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert_eq!(view.handle_key(event), SearchOutcome::Pending);
        assert_eq!(view.pattern, "");
    }

    #[test]
    fn down_and_up_move_the_selection_within_bounds() {
        let mut view = SearchView::new();
        view.push_result(entry("a"));
        view.push_result(entry("b"));

        view.handle_key(key(KeyCode::Down));
        assert_eq!(view.selected, 1);
        view.handle_key(key(KeyCode::Down)); // clamps at the last result
        assert_eq!(view.selected, 1);
        view.handle_key(key(KeyCode::Up));
        assert_eq!(view.selected, 0);
    }

    #[test]
    fn start_clears_previous_results_and_state() {
        let mut view = SearchView::new();
        view.push_result(entry("a"));
        view.finish(true);

        view.start();

        assert!(view.results.is_empty());
        assert!(!view.truncated);
        assert_eq!(view.selected, 0);
    }

    #[test]
    fn esc_reports_cancel() {
        let mut view = SearchView::new();
        assert_eq!(view.handle_key(key(KeyCode::Esc)), SearchOutcome::Cancel);
    }

    #[test]
    fn enter_reports_open() {
        let mut view = SearchView::new();
        assert_eq!(view.handle_key(key(KeyCode::Enter)), SearchOutcome::Open);
    }
}
