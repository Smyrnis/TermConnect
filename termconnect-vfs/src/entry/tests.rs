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
