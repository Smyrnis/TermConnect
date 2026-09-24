use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{List, ListItem, ListState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::widgets::panel_view::{PanelView, Row};

pub fn render_file_list(frame: &mut Frame, area: Rect, panel: &PanelView, is_active: bool) {
    let columns = Columns::for_width(area.width);

    let items: Vec<ListItem> =
        panel.rows().iter().map(|row| ListItem::new(row_label(row, is_selected(panel, row), columns))).collect();

    let mut highlight_style = Style::default().add_modifier(Modifier::REVERSED);
    if !is_active {
        highlight_style = highlight_style.add_modifier(Modifier::DIM);
    }

    let list = List::new(items).highlight_style(highlight_style);

    let mut state = ListState::default();
    if !panel.rows().is_empty() {
        state.select(Some(panel.cursor));
    }

    frame.render_stateful_widget(list, area, &mut state);
}

#[derive(Debug, Clone, Copy)]
struct Columns {
    name_width: usize,
    show_size: bool,
    show_permissions: bool,
}

const SELECTION_MARKER_WIDTH: usize = 2;
const COLUMN_GAP: usize = 1;
const SIZE_WIDTH: usize = 8;
const PERMISSIONS_WIDTH: usize = 9;

impl Columns {
    fn for_width(width: u16) -> Self {
        let width = width as usize;
        let show_permissions = width >= 40;
        let show_size = width >= 28;

        let mut overhead = SELECTION_MARKER_WIDTH;
        if show_size {
            overhead += COLUMN_GAP + SIZE_WIDTH;
        }
        if show_permissions {
            overhead += COLUMN_GAP + PERMISSIONS_WIDTH;
        }

        Columns { name_width: width.saturating_sub(overhead).max(4), show_size, show_permissions }
    }
}

fn is_selected(panel: &PanelView, row: &Row) -> bool {
    match row {
        Row::Entry(entry) => panel.selected.contains(&entry.path),
        Row::Parent => false,
    }
}

fn row_label(row: &Row, selected: bool, columns: Columns) -> String {
    match row {
        Row::Parent => "  ..".to_string(),
        Row::Entry(entry) => {
            let marker = if selected { '*' } else { ' ' };
            let suffix = if entry.is_dir { "/" } else { "" };
            let name = truncate_name(&format!("{}{suffix}", entry.name), columns.name_width);
            let padding = " ".repeat(columns.name_width.saturating_sub(UnicodeWidthStr::width(name.as_str())));

            let mut label = format!("{marker} {name}{padding}");
            if columns.show_size {
                let size = format_size(entry.size, entry.is_dir);
                label = format!("{label} {size:>width$}", width = SIZE_WIDTH);
            }
            if columns.show_permissions {
                label = format!("{label} {}", format_permissions(entry.permissions));
            }
            label
        }
    }
}

pub(crate) fn truncate_name(name: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(name) <= max_width {
        return name.to_string();
    }
    if max_width <= 1 {
        return "\u{2026}".to_string();
    }

    let mut head = String::new();
    let mut width = 0;
    for ch in name.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width - 1 {
            break;
        }
        head.push(ch);
        width += ch_width;
    }
    format!("{head}\u{2026}")
}

pub fn format_size(size: u64, is_dir: bool) -> String {
    if is_dir {
        return "\u{2014}".to_string();
    }
    if size < 1024 {
        return size.to_string();
    }

    const UNITS: [&str; 4] = ["K", "M", "G", "T"];
    let mut value = size as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1}{}", UNITS[unit])
}

pub fn format_permissions(mode: Option<u32>) -> String {
    let Some(mode) = mode else {
        return "-".repeat(9);
    };

    const BITS: [(u32, char); 9] = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];

    BITS.iter().map(|(bit, ch)| if mode & bit != 0 { *ch } else { '-' }).collect()
}

#[cfg(test)]
mod tests;
