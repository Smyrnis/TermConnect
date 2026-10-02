use std::path::Path;

use super::*;

#[test]
fn parses_a_single_host_block() {
    let config = "\
Host production
    HostName server.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_ed25519
";
    let hosts = parse(config, Some(Path::new("/home/test")));

    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "production");
    assert_eq!(hosts[0].host_name.as_deref(), Some("server.example.com"));
    assert_eq!(hosts[0].user.as_deref(), Some("deploy"));
    assert_eq!(hosts[0].port, Some(2222));
}

#[test]
fn skips_wildcard_host_patterns() {
    let config = "\
Host *
    User default_user

Host staging
    HostName staging.example.com
";
    let hosts = parse(config, Some(Path::new("/home/test")));

    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "staging");
}

#[test]
fn one_host_line_with_multiple_aliases_shares_settings() {
    let config = "\
Host a b
    HostName shared.example.com
";
    let hosts = parse(config, Some(Path::new("/home/test")));

    assert_eq!(hosts.len(), 2);
    assert_eq!(hosts[0].name, "a");
    assert_eq!(hosts[1].name, "b");
    assert_eq!(hosts[0].host_name.as_deref(), Some("shared.example.com"));
    assert_eq!(hosts[1].host_name.as_deref(), Some("shared.example.com"));
}

#[test]
fn ignores_comments_and_blank_lines() {
    let config = "\
# a comment
Host production # trailing comment
    HostName server.example.com

";
    let hosts = parse(config, Some(Path::new("/home/test")));

    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "production");
    assert_eq!(hosts[0].host_name.as_deref(), Some("server.example.com"));
}

#[test]
fn unknown_directives_are_ignored() {
    let config = "\
Host production
    ProxyJump bastion
    HostName server.example.com
";
    let hosts = parse(config, Some(Path::new("/home/test")));

    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].host_name.as_deref(), Some("server.example.com"));
}

#[test]
fn identity_files_expand_the_tilde_against_the_given_home() {
    let hosts = parse("Host a\n    IdentityFile ~/.ssh/id\n", Some(Path::new("/home/test")));

    assert_eq!(hosts[0].identity_file, Some(std::path::PathBuf::from("/home/test/.ssh/id")));
}

fn only(config: &str) -> SshConfigHost {
    let mut hosts = parse(config, Some(Path::new("/home/test")));
    assert_eq!(hosts.len(), 1, "{hosts:?}");
    hosts.remove(0)
}

#[test]
fn a_later_wildcard_block_fills_in_what_the_host_block_leaves_unset() {
    let host = only("Host box\n  HostName 10.0.0.2\nHost *\n  User deploy\n  Port 2222\n");

    assert_eq!(host.host_name.as_deref(), Some("10.0.0.2"));
    assert_eq!(host.user.as_deref(), Some("deploy"));
    assert_eq!(host.port, Some(2222));
}

#[test]
fn the_first_value_obtained_wins_like_ssh() {
    assert_eq!(only("Host box\n  User alice\nHost *\n  User deploy\n").user.as_deref(), Some("alice"));
    assert_eq!(only("Host *\n  User deploy\nHost box\n  User alice\n").user.as_deref(), Some("deploy"));
}

#[test]
fn a_repeated_host_block_is_listed_once_and_its_first_value_wins() {
    let host = only("Host box\n  Port 2200\nHost box\n  Port 2300\n  User alice\n");

    assert_eq!(host.port, Some(2200));
    assert_eq!(host.user.as_deref(), Some("alice"));
}

#[test]
fn settings_before_any_host_line_apply_to_every_host() {
    let hosts = parse("User deploy\nHost a\nHost b\n  User alice\n", None);

    assert_eq!(hosts[0].user.as_deref(), Some("deploy"));
    assert_eq!(hosts[1].user.as_deref(), Some("deploy"));
}

#[test]
fn question_mark_patterns_match_a_single_character() {
    let host = only("Host web1\nHost web?\n  Port 2200\nHost web??\n  User nobody\n");

    assert_eq!(host.port, Some(2200));
    assert_eq!(host.user, None);
}

#[test]
fn host_patterns_match_case_insensitively() {
    assert_eq!(only("Host Box\nHost box\n  User alice\n").user.as_deref(), Some("alice"));
}

#[test]
fn a_negated_pattern_excludes_that_host_from_the_block() {
    let hosts = parse("Host box other\nHost * !box\n  User deploy\n", None);

    assert_eq!(hosts[0].user, None);
    assert_eq!(hosts[1].user.as_deref(), Some("deploy"));
}

#[test]
fn hostname_expands_the_alias_token() {
    let host = only("Host box\nHost *\n  HostName %h.corp.example.com\n");

    assert_eq!(host.host_name.as_deref(), Some("box.corp.example.com"));
}

#[test]
fn hostname_keeps_a_literal_percent_written_as_double_percent() {
    assert_eq!(only("Host box\n  HostName a%%b\n").host_name.as_deref(), Some("a%b"));
}

#[test]
fn match_blocks_are_skipped_instead_of_leaking_into_the_previous_host() {
    let host = only("Host box\n  HostName a.example.com\nMatch user root\n  HostName b.example.com\n  Port 2200\n");

    assert_eq!(host.host_name.as_deref(), Some("a.example.com"));
    assert_eq!(host.port, None);
}

#[test]
fn a_host_line_after_a_match_block_starts_matching_again() {
    let host = only("Match all\n  User root\nHost box\n  User alice\n");

    assert_eq!(host.user.as_deref(), Some("alice"));
}

#[test]
fn a_host_blocks_own_identity_file_beats_an_earlier_wildcard_default() {
    let host = only("Host *\n  IdentityFile ~/.ssh/id_default\nHost work\n  IdentityFile ~/.ssh/id_work\n");

    assert_eq!(host.identity_file, Some(std::path::PathBuf::from("/home/test/.ssh/id_work")));
}

#[test]
fn a_wildcard_identity_file_applies_when_the_host_names_none() {
    let host = only("Host work\n  User alice\nHost *\n  IdentityFile ~/.ssh/id_default\n");

    assert_eq!(host.identity_file, Some(std::path::PathBuf::from("/home/test/.ssh/id_default")));
}

#[test]
fn the_first_identity_file_among_blocks_naming_the_host_wins() {
    let host = parse(
        "Host work\n  IdentityFile ~/.ssh/one\nHost work other\n  IdentityFile ~/.ssh/two\n",
        Some(Path::new("/home/test")),
    )
    .remove(0);

    assert_eq!(host.identity_file, Some(std::path::PathBuf::from("/home/test/.ssh/one")));
}
