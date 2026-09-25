use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use porthmos_core::{ProtocolInfo, profiles::ConnectionEntry};

pub fn render_connections_list(
    frame: &mut Frame, area: Rect, entries: &[ConnectionEntry], cursor: usize, connected_names: &HashSet<&str>,
    active_name: Option<&str>, protocols: &[ProtocolInfo],
) {
    let block = Block::default().title("Connections").borders(Borders::ALL);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let column = entries.iter().map(|entry| protocol_label(entry, protocols).chars().count()).max().unwrap_or(0);
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
            let protocol = protocol_label(entry, protocols);
            ListItem::new(format!(
                "{marker}{protocol:<column$} {} ({}@{}:{})",
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

fn protocol_label<'a>(entry: &'a ConnectionEntry, protocols: &'a [ProtocolInfo]) -> &'a str {
    protocols.iter().find(|info| info.id == entry.protocol).map_or(entry.protocol.as_str(), |info| info.display_name)
}

#[cfg(test)]
mod tests;
