use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::connection::ConnectionEntry;

pub fn render_connections_list(
    frame: &mut Frame,
    area: Rect,
    entries: &[ConnectionEntry],
    cursor: usize,
    active_name: Option<&str>,
) {
    let block = Block::default().title("Connections").borders(Borders::ALL);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            let marker = if Some(entry.name.as_str()) == active_name {
                "* "
            } else {
                "  "
            };
            ListItem::new(format!(
                "{marker}{} ({}@{}:{})",
                entry.name, entry.username, entry.host, entry.port
            ))
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default();
    if !entries.is_empty() {
        state.select(Some(cursor));
    }

    frame.render_stateful_widget(list, inner, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_connection_names() {
        let entries = vec![ConnectionEntry {
            name: "production".to_string(),
            host: "server.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
        }];

        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_connections_list(frame, frame.area(), &entries, 0, None))
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("production"));
        assert!(content.contains("deploy@server.example.com:22"));
    }
}
