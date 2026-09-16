use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{List, ListItem, ListState};

use crate::tui::panels::{PanelState, Row};

pub fn render_file_list(frame: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    let columns = Columns::for_width(area.width);

    let items: Vec<ListItem> = panel
        .rows()
        .iter()
        .map(|row| ListItem::new(row_label(row, is_selected(panel, row), columns)))
        .collect();

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

/// Which columns fit in `area.width`, and how wide the flexible name
/// column gets once the fixed-width ones are accounted for. Permissions
/// drops first (below 40 columns), then size (below 28); name is never
/// dropped.
#[derive(Debug, Clone, Copy)]
struct Columns {
    name_width: usize,
    show_size: bool,
    show_permissions: bool,
}

const SIZE_WIDTH: usize = 8;
const PERMISSIONS_WIDTH: usize = 9;

impl Columns {
    fn for_width(width: u16) -> Self {
        let width = width as usize;
        let show_permissions = width >= 40;
        let show_size = width >= 28;

        let mut overhead = 2; // selection marker + one space
        if show_size {
            overhead += 1 + SIZE_WIDTH;
        }
        if show_permissions {
            overhead += 1 + PERMISSIONS_WIDTH;
        }

        Columns {
            name_width: width.saturating_sub(overhead).max(4),
            show_size,
            show_permissions,
        }
    }
}

fn is_selected(panel: &PanelState, row: &Row) -> bool {
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

            let mut label = format!("{marker} {name:<width$}", width = columns.name_width);
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

fn truncate_name(name: &str, max_width: usize) -> String {
    if name.chars().count() <= max_width {
        return name.to_string();
    }
    if max_width <= 1 {
        return "\u{2026}".to_string();
    }
    let head: String = name.chars().take(max_width - 1).collect();
    format!("{head}\u{2026}")
}

/// Human-readable size: raw byte count under 1024, then one decimal place
/// per unit above that (`4.2K`, `1.3M`, …). Directories show an em dash —
/// their reported "size" isn't meaningful to a user.
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

/// Renders the low 9 bits of a Unix mode as `rwxr-xr-x`. `None` (mode
/// unknown) renders as nine dashes rather than panicking or omitting the
/// column, so the layout stays stable.
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

    BITS.iter()
        .map(|(bit, ch)| if mode & bit != 0 { *ch } else { '-' })
        .collect()
}

#[cfg(test)]
#[path = "../../tests/tui/file_list_test.rs"]
mod tests;
