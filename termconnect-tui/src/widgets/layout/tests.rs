use ratatui::layout::Rect;

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
