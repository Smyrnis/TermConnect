use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::connection::ConnectionEntry;

/// Renders the connections list, distinguishing "connected" (this host has
/// a live session, `connected_names`) from "active" (that session is the
/// one currently focused, `active_name`) — with multiple simultaneous
/// sessions, a host can be connected without being the active one, and the
/// user needs to be able to tell both states apart at a glance.
pub fn render_connections_list(
    frame: &mut Frame,
    area: Rect,
    entries: &[ConnectionEntry],
    cursor: usize,
    connected_names: &HashSet<&str>,
    active_name: Option<&str>,
) {
    let block = Block::default().title("Connections").borders(Borders::ALL);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = entries
        .iter()
        .map(|entry| {
            let is_active = Some(entry.name.as_str()) == active_name;
            let is_connected = connected_names.contains(entry.name.as_str());
            let marker = if is_active {
                "* "
            } else if is_connected {
                "+ "
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
            .draw(|frame| {
                render_connections_list(frame, frame.area(), &entries, 0, &HashSet::new(), None)
            })
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

    fn entry(name: &str) -> ConnectionEntry {
        ConnectionEntry {
            name: name.to_string(),
            host: format!("{name}.example.com"),
            port: 22,
            username: "user".to_string(),
            identity_file: None,
        }
    }

    /// Renders into rows of plain text (one `String` per terminal row) so
    /// assertions can check which marker appears on which entry's line,
    /// rather than just "somewhere in the whole buffer".
    fn render_rows(
        entries: &[ConnectionEntry],
        connected: &HashSet<&str>,
        active: Option<&str>,
    ) -> Vec<String> {
        let width = 60;
        let backend = TestBackend::new(width, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_connections_list(frame, frame.area(), entries, 0, connected, active)
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn multiple_connected_sessions_all_show_as_connected() {
        let entries = vec![entry("a"), entry("b"), entry("c")];
        let connected: HashSet<&str> = ["a", "b"].into_iter().collect();

        let rows = render_rows(&entries, &connected, Some("a"));
        let full_text = rows.join("\n");

        // "a" is connected AND active: the "*" marker.
        assert!(full_text.contains("* a ("));
        // "b" is connected but not active: the "+" marker.
        assert!(full_text.contains("+ b ("));
        // "c" is neither connected nor active: no marker.
        assert!(full_text.contains("  c ("));
        assert!(!full_text.contains("* c ("));
        assert!(!full_text.contains("+ c ("));
    }
}
