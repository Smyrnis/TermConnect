use std::collections::HashSet;

use porthmos_core::{
    ProtocolInfo,
    connections_tree::TreeRow,
    profiles::{ConnectionEntry, ConnectionSource},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::widgets::connections_view::ConnectionsView;

pub fn render_connections_list(
    frame: &mut Frame, area: Rect, view: &ConnectionsView, connected_names: &HashSet<&str>, active_name: Option<&str>,
    protocols: &[ProtocolInfo],
) {
    let mut block = Block::default().title("Connections").borders(Borders::ALL);
    if let Some(status) = view.filter_status(usize::from(area.width.saturating_sub(2))) {
        block = block.title_bottom(status);
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let entries = view.entries();
    let column = entries.iter().map(|entry| protocol_label(entry, protocols).chars().count()).max().unwrap_or(0);
    let items: Vec<ListItem> = view
        .rows()
        .iter()
        .map(|row| match row {
            TreeRow::Group { name, depth, count, expanded, .. } => {
                let arrow = if *expanded { '\u{25BE}' } else { '\u{25B8}' };
                ListItem::new(format!("{}{arrow} {name} ({count})", indent(*depth)))
            }
            TreeRow::Connection { index, depth } => {
                let entry = &entries[*index];
                ListItem::new(connection_line(entry, *depth, column, connected_names, active_name, protocols))
            }
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default();
    if !view.rows().is_empty() {
        state.select(Some(view.cursor));
    }

    frame.render_stateful_widget(list, inner, &mut state);
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn connection_line<'a>(
    entry: &'a ConnectionEntry, depth: usize, column: usize, connected_names: &HashSet<&str>,
    active_name: Option<&str>, protocols: &'a [ProtocolInfo],
) -> Line<'a> {
    let indent = indent(depth);
    let reason = match entry.source {
        ConnectionSource::MissingSshHost => Some("not in ~/.ssh/config"),
        ConnectionSource::ShadowedSshHost => Some("hidden by a saved connection"),
        ConnectionSource::Profile | ConnectionSource::SshConfig => None,
    };
    if let Some(reason) = reason {
        return Line::from(format!("{indent}\u{26A0} {} ({reason})", entry.name));
    }

    let marker = if Some(entry.name.as_str()) == active_name {
        "* "
    } else if connected_names.contains(entry.name.as_str()) {
        "+ "
    } else {
        "  "
    };
    let protocol = protocol_label(entry, protocols);
    let mut spans = vec![Span::raw(format!(
        "{indent}{marker}{protocol:<column$} {} ({}@{}:{})",
        entry.name, entry.username, entry.host, entry.port
    ))];
    let dimmed = Style::default().add_modifier(Modifier::DIM);
    spans.extend(entry.tags.iter().map(|tag| Span::styled(format!(" #{tag}"), dimmed)));
    Line::from(spans)
}

fn protocol_label<'a>(entry: &'a ConnectionEntry, protocols: &'a [ProtocolInfo]) -> &'a str {
    protocols.iter().find(|info| info.id == entry.protocol).map_or(entry.protocol.as_str(), |info| info.display_name)
}

#[cfg(test)]
mod tests;
