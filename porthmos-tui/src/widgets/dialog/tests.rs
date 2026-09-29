use super::*;

#[test]
fn content_width_grows_with_the_longest_line_but_stays_clamped() {
    assert_eq!(content_width(&["short"]), 20);
    assert_eq!(content_width(&[&"x".repeat(37)]), 41);
    assert_eq!(content_width(&[&"x".repeat(200)]), 76);
}

#[test]
fn a_dialog_outcome_never_prints_submitted_values() {
    let form = DialogOutcome::FormSubmitted(vec![("password", "hunter2".to_string())]);
    let text = DialogOutcome::Submitted("s3cret".to_string());

    assert!(!format!("{form:?}").contains("hunter2"));
    assert!(!format!("{text:?}").contains("s3cret"));
    assert!(format!("{form:?}").contains("password"));
}
