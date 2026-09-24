use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::content_width;
use porthmos_core::transfer::{conflicts::Resolution, plan::ExistingFile};

use crate::widgets::file_list::format_size;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

pub struct ConflictDialog {
    pub file_name: String,
    pub existing: Option<ExistingFile>,
    pub partial: Option<ExistingFile>,
    pub new_size: u64,
    pub new_modified: Option<u64>,
    pub index: usize,
    pub total: usize,
    pub apply_to_rest: bool,
    pub now: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictOutcome {
    Pending,
    Resolved { resolution: Option<Resolution>, apply_to_rest: bool },
}

impl ConflictDialog {
    fn offers_resume(&self) -> bool {
        self.partial.is_some() && !self.existing_is_dir()
    }

    fn existing_is_dir(&self) -> bool {
        self.existing.is_some_and(|existing| existing.is_dir)
    }

    pub fn remaining_after_this(&self) -> usize {
        self.total.saturating_sub(self.index + 1)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ConflictOutcome {
        let apply_to_rest = self.apply_to_rest;
        let answer = |resolution| ConflictOutcome::Resolved { resolution: Some(resolution), apply_to_rest };
        let with_shortcut_modifier = key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('c' | 'C') | KeyCode::Esc => {
                ConflictOutcome::Resolved { resolution: None, apply_to_rest: false }
            }
            _ if with_shortcut_modifier => ConflictOutcome::Pending,
            KeyCode::Char('u' | 'U') if self.offers_resume() => answer(Resolution::Resume),
            KeyCode::Char('o' | 'O') if !self.existing_is_dir() => answer(Resolution::Overwrite),
            KeyCode::Char('s' | 'S') => answer(Resolution::Skip),
            KeyCode::Char('r' | 'R') if self.existing.is_some() => answer(Resolution::Rename),
            KeyCode::Char('a' | 'A') if self.remaining_after_this() > 0 => {
                self.apply_to_rest = !self.apply_to_rest;
                ConflictOutcome::Pending
            }
            _ => ConflictOutcome::Pending,
        }
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(existing) = self.existing {
            lines.push(if existing.is_dir {
                "Existing  folder".to_string()
            } else {
                format!(
                    "Existing  {:>8}   modified {}",
                    format_size(existing.size, false),
                    age_text(existing.modified, self.now)
                )
            });
        }
        if let Some(partial) = self.partial {
            lines.push(format!(
                "Partial   {:>8} of {}   modified {}",
                format_size(partial.size, false),
                format_size(self.new_size, false),
                age_text(partial.modified, self.now)
            ));
        }
        lines.push(format!(
            "New       {:>8}   modified {}",
            format_size(self.new_size, false),
            age_text(self.new_modified, self.now)
        ));
        lines.push(String::new());
        let mut options = Vec::new();
        if self.offers_resume() {
            options.push("Res[u]me");
        }
        if !self.existing_is_dir() {
            options.push(if self.existing.is_some() { "[O]verwrite" } else { "Start [o]ver" });
        }
        options.push("[S]kip");
        if self.existing.is_some() {
            options.push("[R]ename");
        }
        options.push("[C]ancel copy");
        lines.push(options.join("  "));
        let remaining = self.remaining_after_this();
        if remaining > 0 {
            let mark = if self.apply_to_rest { "x" } else { " " };
            lines.push(format!("[A] [{mark}] same answer for the other {remaining}"));
        }
        lines
    }
}

pub fn format_age(seconds_ago: i64) -> String {
    let (count, unit) = if seconds_ago < MINUTE {
        return "just now".to_string();
    } else if seconds_ago < HOUR {
        (seconds_ago / MINUTE, "minute")
    } else if seconds_ago < DAY {
        (seconds_ago / HOUR, "hour")
    } else {
        (seconds_ago / DAY, "day")
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

fn age_text(modified: Option<u64>, now: u64) -> String {
    match modified {
        Some(modified) => format_age(now as i64 - modified as i64),
        None => "unknown".to_string(),
    }
}

pub fn render_conflict(frame: &mut Frame, area: Rect, dialog: &ConflictDialog) {
    let state = if dialog.existing.is_some() { "already exists" } else { "was partly copied before" };
    let title = format!("\"{}\" {state} ({} of {})", dialog.file_name, dialog.index + 1, dialog.total);
    let lines = dialog.lines();
    let mut measured: Vec<&str> = lines.iter().map(String::as_str).collect();
    measured.push(&title);
    let width = content_width(&measured).min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let [popup] = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center).areas(area);
    let [popup] = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center).areas(popup);

    let block = Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines.join("\n")).block(block), popup);
}

#[cfg(test)]
mod tests;
