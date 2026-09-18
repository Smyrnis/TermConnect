use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub fn split_panels(area: Rect) -> (Rect, Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    (columns[0], columns[1])
}

/// Splits the whole frame into a one-line title bar, the main content
/// area, and a one-line status bar along the bottom.
pub fn split_frame(area: Rect) -> (Rect, Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    (rows[0], rows[1], rows[2])
}

/// Splits a remote-panel area into a one-line session tab strip and the
/// remaining panel area, used when more than one session is connected.
pub fn split_remote_with_tabs(area: Rect) -> (Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    (rows[0], rows[1])
}

#[cfg(test)]
#[path = "../../tests/tui/layout_test.rs"]
mod tests;
