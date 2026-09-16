use super::*;

#[test]
fn content_width_grows_with_the_longest_line_but_stays_clamped() {
    assert_eq!(content_width(&["short"]), 20); // 5 + 4 = 9, clamped up to the minimum
    assert_eq!(content_width(&[&"x".repeat(37)]), 41); // 37 + 4, within range
    assert_eq!(content_width(&[&"x".repeat(200)]), 76); // clamped to the maximum
}
