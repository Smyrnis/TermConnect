use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};

use crate::tui::input::{ALL_ACTIONS, KeyBindings, format_key_spec};

pub fn render_help(frame: &mut Frame, area: Rect, bindings: &KeyBindings) {
    let items: Vec<ListItem> = ALL_ACTIONS
        .iter()
        .map(|action| {
            let key = bindings
                .key_for(*action)
                .map(format_key_spec)
                .unwrap_or_else(|| "(unbound)".to_string());
            ListItem::new(format!("{key:<10} {}", action.name()))
        })
        .collect();

    let width = 40u16.min(area.width);
    let height = (items.len() as u16 + 2).clamp(3, area.height.saturating_sub(2).max(3));
    let popup = centered_popup(area, width, height);

    let block = Block::default()
        .title("Help \u{2014} any key to close")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let list = List::new(items).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(list, popup);
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
    use crate::tui::input::KeyBindings;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_every_action_with_its_bound_key() {
        let bindings = KeyBindings::defaults();
        let backend = TestBackend::new(50, 25);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_help(frame, frame.area(), &bindings))
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("quit"));
        assert!(content.contains("F10"));
    }
}
