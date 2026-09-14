use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub fn split_panels(area: Rect) -> (Rect, Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    (columns[0], columns[1])
}

/// Splits the whole frame into the main panel area and a one-line status
/// bar along the bottom.
pub fn split_frame(area: Rect) -> (Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
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
    fn split_frame_reserves_one_line_for_the_status_bar() {
        let area = Rect::new(0, 0, 100, 40);
        let (main, status) = split_frame(area);

        assert_eq!(main.height, 39);
        assert_eq!(status.height, 1);
        assert_eq!(status.y, 39);
    }
}
