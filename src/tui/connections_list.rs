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
    frame: &mut Frame, area: Rect, entries: &[ConnectionEntry], cursor: usize, connected_names: &HashSet<&str>,
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
            ListItem::new(format!("{marker}{} ({}@{}:{})", entry.name, entry.username, entry.host, entry.port))
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
#[path = "../../tests/tui/connections_list_test.rs"]
mod tests;
