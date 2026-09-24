use super::*;

#[test]
fn shell_quote_wraps_plain_text_in_single_quotes() {
    assert_eq!(shell_quote("simple"), "'simple'");
}

#[test]
fn shell_quote_escapes_embedded_single_quotes() {
    assert_eq!(shell_quote("it's here"), "'it'\\''s here'");
}

#[test]
fn shell_quote_preserves_other_shell_metacharacters_literally_inside_quotes() {
    assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
}
