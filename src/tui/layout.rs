use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub fn split_panels(area: Rect) -> (Rect, Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    (columns[0], columns[1])
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
}
