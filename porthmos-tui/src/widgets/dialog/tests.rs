use super::*;

#[test]
fn content_width_grows_with_the_longest_line_but_stays_clamped() {
    assert_eq!(content_width(&["short"]), 20);
    assert_eq!(content_width(&[&"x".repeat(37)]), 41);
    assert_eq!(content_width(&[&"x".repeat(200)]), 76);
}
