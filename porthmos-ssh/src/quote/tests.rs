use super::*;

#[test]
fn plain_names_are_single_quoted() {
    assert_eq!(shell_quote("a b.txt"), "'a b.txt'");
}

#[test]
fn single_quotes_are_closed_escaped_and_reopened() {
    assert_eq!(shell_quote("it's"), r"'it'\''s'");
}

#[test]
fn shell_syntax_stays_literal() {
    assert_eq!(shell_quote("$HOME `id` -rf *"), "'$HOME `id` -rf *'");
}

#[test]
fn an_empty_value_is_an_empty_word() {
    assert_eq!(shell_quote(""), "''");
}
