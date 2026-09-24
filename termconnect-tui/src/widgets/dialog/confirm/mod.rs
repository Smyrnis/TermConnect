use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct ConfirmDialog {
    pub message: String,
    pub focus: ConfirmFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmFocus {
    Yes,
    No,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmOutcome {
    Pending,
    Confirmed,
    Cancelled,
}

impl ConfirmDialog {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), focus: ConfirmFocus::No }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ConfirmOutcome {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => ConfirmOutcome::Confirmed,
            KeyCode::Char('n') | KeyCode::Char('N') => ConfirmOutcome::Cancelled,
            KeyCode::Esc => ConfirmOutcome::Cancelled,
            KeyCode::Tab | KeyCode::Left | KeyCode::Right => {
                self.focus = match self.focus {
                    ConfirmFocus::Yes => ConfirmFocus::No,
                    ConfirmFocus::No => ConfirmFocus::Yes,
                };
                ConfirmOutcome::Pending
            }
            KeyCode::Enter => match self.focus {
                ConfirmFocus::Yes => ConfirmOutcome::Confirmed,
                ConfirmFocus::No => ConfirmOutcome::Cancelled,
            },
            _ => ConfirmOutcome::Pending,
        }
    }
}

pub fn render_confirm(frame: &mut Frame, area: Rect, dialog: &ConfirmDialog) {
    let popup = centered_popup(area, super::content_width(&[dialog.message.as_str(), "[y] Yes   [n] No"]), 5);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));

    let yes_style = if dialog.focus == ConfirmFocus::Yes {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let no_style = if dialog.focus == ConfirmFocus::No {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };

    let text = Text::from(vec![
        Line::from(dialog.message.as_str()),
        Line::from(""),
        Line::from(vec![Span::styled("[y] Yes", yes_style), Span::raw("   "), Span::styled("[n] No", no_style)]),
    ]);

    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Center);

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
