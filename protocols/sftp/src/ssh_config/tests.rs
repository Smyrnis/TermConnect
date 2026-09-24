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
