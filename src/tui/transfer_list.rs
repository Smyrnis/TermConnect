use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    transfer::{
        Direction,
        rows::{QueueRow, RowKind, RowState, percent_of},
    },
    tui::file_list::{format_size, truncate_name},
};

const ARROW_WIDTH: usize = 2;
const PERCENT_WIDTH: usize = 4;
const MIN_LABEL_WIDTH: usize = 8;

struct Columns {
    label: usize,
    amount: Option<usize>,
    show_percent: bool,
}

pub fn render_transfer_list(frame: &mut Frame, area: Rect, rows: &[QueueRow], cursor: usize, copy_key: Option<&str>) {
    let block = Block::default().title(format!("Transfers ({})", rows.len())).borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if rows.is_empty() {
        let message = match copy_key {
            Some(key) => format!("No transfers \u{2014} {key} copies the selected file or folder"),
            None => "No transfers yet".to_string(),
        };
        frame.render_widget(Paragraph::new(message), inner);
        return;
    }

    let columns = columns_for(rows, inner.width as usize);
    let items: Vec<ListItem> = rows.iter().map(|row| ListItem::new(row_text(row, &columns))).collect();
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default();
    state.select(Some(cursor.min(rows.len() - 1)));
    frame.render_stateful_widget(list, inner, &mut state);
}

fn columns_for(rows: &[QueueRow], width: usize) -> Columns {
    let state_part = 2 + rows.iter().map(|row| UnicodeWidthStr::width(state_text(row.state).as_str())).max().unwrap_or(0);
    let amount_width = rows.iter().map(|row| amount_text(row).len()).max().unwrap_or(0);
    let amount_part = 1 + amount_width;
    let percent_part = 1 + PERCENT_WIDTH;
    let label_room = |fixed: usize| width.checked_sub(ARROW_WIDTH + fixed).filter(|room| *room >= MIN_LABEL_WIDTH);

    if let Some(label) = label_room(amount_part + percent_part + state_part) {
        Columns { label, amount: Some(amount_width), show_percent: true }
    } else if let Some(label) = label_room(percent_part + state_part) {
        Columns { label, amount: None, show_percent: true }
    } else {
        Columns { label: width.saturating_sub(ARROW_WIDTH + state_part).max(1), amount: None, show_percent: false }
    }
}

fn row_text(row: &QueueRow, columns: &Columns) -> String {
    let arrow = match row.direction {
        Direction::Upload => '\u{2191}',
        Direction::Download => '\u{2193}',
    };
    let label = truncate_name(&row.label, columns.label);
    let padding = " ".repeat(columns.label.saturating_sub(UnicodeWidthStr::width(label.as_str())));
    let mut text = format!("{arrow} {label}{padding}");
    if let Some(amount_width) = columns.amount {
        text.push_str(&format!(" {:>amount_width$}", amount_text(row)));
    }
    if columns.show_percent {
        text.push_str(&format!(" {:>PERCENT_WIDTH$}", percent_text(row)));
    }
    text.push_str(&format!("  {}", state_text(row.state)));
    text
}

fn amount_text(row: &QueueRow) -> String {
    match row.kind {
        RowKind::Batch(_) => format!("{}/{} files", row.files_done, row.files_total),
        RowKind::Single(_) => format_size(row.bytes_total, false),
        RowKind::Scan(_) => String::new(),
    }
}

fn percent_text(row: &QueueRow) -> String {
    match row.kind {
        RowKind::Scan(_) => String::new(),
        _ => format!("{}%", percent_of(row.bytes_done, row.bytes_total)),
    }
}
fn state_text(state: RowState) -> String {
    match state {
        RowState::Scanning => "scanning\u{2026}".to_string(),
        RowState::AwaitingAnswer => "waiting for you".to_string(),
        RowState::Running => "running".to_string(),
        RowState::Queued => "queued".to_string(),
        RowState::Done => "done".to_string(),
        RowState::PartlyFailed(failed) => format!("done, {failed} failed"),
        RowState::Failed => "failed".to_string(),
        RowState::Cancelled => "cancelled".to_string(),
    }
}

#[cfg(test)]
#[path = "../../tests/tui/transfer_list_test.rs"]
mod tests;
