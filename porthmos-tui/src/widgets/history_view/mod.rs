use chrono::Local;
use porthmos_core::{
    history::{self, HistoryEntry, HistoryResult},
    transfer::Direction,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::widgets::{
    file_list::format_size,
    filter_line::{self, FilterLine},
};

#[derive(Default)]
pub struct HistoryView {
    entries: Vec<HistoryEntry>,
    visible: Vec<usize>,
    filter: Option<String>,
    editing_filter: bool,
    cursor: usize,
}

impl HistoryView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace(&mut self, entries: Vec<HistoryEntry>) {
        let anchor = self.selected().cloned();
        self.entries = entries;
        self.refilter();
        let position = anchor.and_then(|anchor| self.rows().position(|entry| *entry == anchor));
        if let Some(position) = position {
            self.cursor = position;
        }
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn matched(&self) -> usize {
        self.visible.len()
    }

    pub fn rows(&self) -> impl Iterator<Item = &HistoryEntry> + '_ {
        self.visible.iter().map(|index| &self.entries[*index])
    }

    pub fn selected(&self) -> Option<&HistoryEntry> {
        self.visible.get(self.cursor).map(|index| &self.entries[*index])
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn go_to_top(&mut self) {
        self.cursor = 0;
    }

    fn refilter(&mut self) {
        let filter = self.filter.as_deref();
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| filter.is_none_or(|text| history::matches(entry, text)))
            .map(|(index, _)| index)
            .collect();
        self.cursor = self.cursor.min(self.visible.len().saturating_sub(1));
    }

    fn set_filter(&mut self, text: Option<String>) {
        self.filter = text.filter(|text| !text.is_empty());
        self.refilter();
    }
}

impl FilterLine for HistoryView {
    fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    fn editing_filter(&self) -> bool {
        self.editing_filter
    }

    fn start_filter(&mut self) {
        self.editing_filter = true;
    }

    fn finish_filter(&mut self) {
        self.editing_filter = false;
    }

    fn type_filter(&mut self, character: char) {
        let mut text = self.filter.clone().unwrap_or_default();
        text.push(character);
        self.set_filter(Some(text));
    }

    fn erase_filter(&mut self) {
        let mut text = self.filter.clone().unwrap_or_default();
        text.pop();
        self.set_filter(Some(text));
    }

    fn clear_filter(&mut self) {
        self.editing_filter = false;
        self.set_filter(None);
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }
}

pub fn result_marker(result: &HistoryResult) -> char {
    match result {
        HistoryResult::Failed => '\u{2717}',
        HistoryResult::PartlyFailed { .. } => '!',
        HistoryResult::Done | HistoryResult::Cancelled | HistoryResult::Interrupted => ' ',
    }
}

fn result_style(result: &HistoryResult) -> Style {
    match result {
        HistoryResult::Failed => Style::default().fg(Color::Red),
        HistoryResult::PartlyFailed { .. } => Style::default().fg(Color::Yellow),
        HistoryResult::Cancelled | HistoryResult::Interrupted => Style::default().add_modifier(Modifier::DIM),
        HistoryResult::Done => Style::default(),
    }
}

fn is_unprintable(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{61c}'
        )
}

pub fn printable(text: &str) -> String {
    text.chars().map(|character| if is_unprintable(character) { '?' } else { character }).collect()
}

fn row_text(entry: &HistoryEntry) -> String {
    let time = entry.finished_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
    let arrow = match entry.direction {
        Direction::Upload => '\u{2191}',
        Direction::Download => '\u{2193}',
    };
    format!(
        "{time} {} {arrow} {}  {}  {}/{}  {}",
        result_marker(&entry.result),
        printable(&entry.label),
        printable(&entry.connection),
        entry.files_done,
        entry.files_total,
        format_size(entry.bytes, false),
    )
}

pub fn render_history(frame: &mut Frame, area: Rect, view: &HistoryView) {
    let mut block = Block::default().title(format!("History ({})", view.total())).borders(Borders::ALL);
    let inner_width = usize::from(area.width.saturating_sub(2));
    if let Some(status) =
        filter_line::status(view.editing_filter(), view.filter(), view.matched(), view.total(), inner_width)
    {
        block = block.title_bottom(status);
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if view.total() == 0 {
        frame.render_widget(Paragraph::new("No transfers yet"), inner);
        return;
    }
    if view.matched() == 0 {
        frame.render_widget(Paragraph::new("No matches"), inner);
        return;
    }

    let items: Vec<ListItem> =
        view.rows().map(|entry| ListItem::new(row_text(entry)).style(result_style(&entry.result))).collect();
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    state.select(Some(view.cursor()));
    frame.render_stateful_widget(list, inner, &mut state);
}

#[cfg(test)]
mod tests;
