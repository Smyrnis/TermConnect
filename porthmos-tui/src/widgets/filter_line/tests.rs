use super::*;

#[test]
fn nothing_is_shown_without_a_filter_or_the_line() {
    assert_eq!(status(false, None, 3, 3, 40), None);
}

#[test]
fn the_line_shows_the_caret_and_the_count() {
    assert_eq!(status(true, Some("po"), 2, 4, 40).as_deref(), Some("/po\u{2588} (2 of 4)"));
    assert_eq!(status(true, None, 4, 4, 40).as_deref(), Some("/\u{2588} (4 of 4)"));
}

#[test]
fn a_kept_filter_is_labelled() {
    assert_eq!(status(false, Some("po"), 2, 4, 40).as_deref(), Some("filter: po (2 of 4)"));
}

#[test]
fn long_text_keeps_its_end_within_the_width() {
    let shown = status(true, Some("a_really_long_filter_text"), 0, 4, 22).unwrap();
    assert_eq!(shown.chars().count(), 22);
    assert!(shown.starts_with("/\u{2026}") && shown.ends_with("_text\u{2588} (0 of 4)"), "{shown}");
}
