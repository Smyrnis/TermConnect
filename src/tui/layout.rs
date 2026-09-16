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
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
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
mod tests {
    use super::*;

    #[test]
    fn splits_area_into_two_equal_halves() {
        let area = Rect::new(0, 0, 100, 40);
        let (local, remote) = split_panels(area);

        assert_eq!(local.x, 0);
        assert_eq!(local.width, 50);
        assert_eq!(local.height, 40);

        assert_eq!(remote.x, 50);
        assert_eq!(remote.width, 50);
        assert_eq!(remote.height, 40);
    }

    #[test]
    fn split_frame_reserves_one_line_each_for_title_and_status_bars() {
        let area = Rect::new(0, 0, 100, 40);
        let (title, main, status) = split_frame(area);

        assert_eq!(title.height, 1);
        assert_eq!(title.y, 0);

        assert_eq!(main.height, 38);
        assert_eq!(main.y, 1);

        assert_eq!(status.height, 1);
        assert_eq!(status.y, 39);
    }
}
