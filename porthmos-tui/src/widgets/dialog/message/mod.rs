use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct MessageDialog {
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageOutcome {
    Pending,
    Closed,
}

impl MessageDialog {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self { title: title.into(), message: message.into() }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> MessageOutcome {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => MessageOutcome::Closed,
            _ => MessageOutcome::Pending,
        }
    }
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    let mut lines = 1;
    let mut used = 0;
    for word in text.split_whitespace() {
        let length = word.chars().count();
        if used == 0 {
            used = length;
        } else if used + 1 + length <= width {
            used += 1 + length;
        } else {
            lines += 1;
            used = length;
        }
        while used > width {
            lines += 1;
            used -= width;
        }
    }
    lines
}

fn wrapped_total(text: &str, width: usize) -> usize {
    text.lines().map(|line| wrapped_line_count(line, width)).sum::<usize>().max(1)
}

pub fn render_message(frame: &mut Frame, area: Rect, dialog: &MessageDialog) {
    let longest_line = dialog.message.lines().map(|line| line.chars().count()).max().unwrap_or(0).min(200) as u16;
    let width = super::content_width(&[dialog.title.as_str(), "[Enter] OK"])
        .max(longest_line + 4)
        .clamp(50, 76)
        .min(area.width);
    let inner_width = usize::from(width.saturating_sub(2)).max(1);
    let lines = wrapped_total(&dialog.message, inner_width) as u16;
    let height = (lines + 4).min(area.height);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    let block = Block::default()
        .title(dialog.title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let mut text: Vec<Line> = dialog.message.lines().map(Line::from).collect();
    text.push(Line::from(""));
    text.push(Line::from("[Enter] OK"));
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(text).block(block).wrap(Wrap { trim: true }), popup);
}

#[cfg(test)]
mod tests;
