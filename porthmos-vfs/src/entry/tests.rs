use super::*;

#[test]
fn join_adds_a_separator_when_missing() {
    assert_eq!(join_remote("/home/user", "file.txt"), "/home/user/file.txt");
}

#[test]
fn join_does_not_double_the_separator() {
    assert_eq!(join_remote("/home/user/", "file.txt"), "/home/user/file.txt");
}

#[test]
fn join_handles_root() {
    assert_eq!(join_remote("/", "etc"), "/etc");
}

#[test]
fn partial_transfers_are_named_with_the_part_suffix() {
    assert_eq!(PART_SUFFIX, ".part");
    assert_eq!(format!("a.txt{PART_SUFFIX}"), "a.txt.part");
}
