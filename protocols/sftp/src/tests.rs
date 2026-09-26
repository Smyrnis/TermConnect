use std::path::Path;

use porthmos_vfs::{Answer, Environment, ErrorKind, Prompter, Protocol, Question, Target};

use super::*;

#[test]
fn discover_fills_missing_fields_from_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".ssh")).unwrap();
    std::fs::write(dir.path().join(".ssh/config"), "Host box\n  HostName 10.0.0.2\n  IdentityFile ~/.ssh/id\n")
        .unwrap();
    let env = Environment { home: Some(dir.path().into()), user: Some("alice".into()), path: None };

    let targets = Sftp.discover(&env).unwrap();

    assert_eq!(targets.len(), 1);
    assert_eq!((targets[0].name.as_str(), targets[0].host.as_str()), ("box", "10.0.0.2"));
    assert_eq!((targets[0].username.as_str(), targets[0].port), ("alice", 22));
    assert_eq!(targets[0].option("identity_file"), Some(dir.path().join(".ssh/id").to_str().unwrap()));
}

#[test]
fn discover_without_a_home_finds_nothing() {
    assert!(Sftp.discover(&Environment::default()).unwrap().is_empty());
}

#[test]
fn discover_falls_back_to_root_when_no_user_is_known() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".ssh")).unwrap();
    std::fs::write(dir.path().join(".ssh/config"), "Host box\n").unwrap();
    let env = Environment { home: Some(dir.path().into()), user: None, path: None };

    let targets = Sftp.discover(&env).unwrap();

    assert_eq!(targets[0].username, "root");
}

#[test]
fn discover_uses_the_alias_as_the_host_when_no_hostname_is_set() {
    let (_dir, env) = home_with_ssh_config("Host box\n  User deploy\n");

    let targets = Sftp.discover(&env).unwrap();

    assert_eq!((targets[0].name.as_str(), targets[0].host.as_str()), ("box", "box"));
}

#[test]
fn the_shell_for_a_discovered_host_without_a_hostname_goes_through_its_alias() {
    let (_dir, env) = home_with_ssh_config("Host box\n  User deploy\n");
    let target = Sftp.discover(&env).unwrap().remove(0);

    assert_eq!(shell_args(&target, &env), ["-o", "HostName=box", "-p", "22", "-l", "deploy", "box"]);
}

#[test]
fn discover_applies_wildcard_defaults_and_hostname_tokens() {
    let (_dir, env) =
        home_with_ssh_config("Host box\nHost *\n  HostName %h.corp.example.com\n  User deploy\n  Port 2222\n");

    let target = Sftp.discover(&env).unwrap().remove(0);

    assert_eq!(target.host, "box.corp.example.com");
    assert_eq!((target.username.as_str(), target.port), ("deploy", 2222));
    assert_eq!(shell_args(&target, &env), ["-o", "HostName=box.corp.example.com", "-p", "2222", "-l", "deploy", "box"]);
}

#[test]
fn shell_command_is_always_available_for_sftp() {
    let target = Target {
        name: "n".into(),
        host: "h".into(),
        port: 22,
        username: "u".into(),
        password: None,
        options: Default::default(),
    };
    let env = Environment { home: None, user: None, path: Some(Path::new("/nonexistent").as_os_str().to_owned()) };

    let invocation = Sftp.shell_command(&target, &env).unwrap();

    assert_eq!(invocation.program, "ssh");
}

fn home_with_ssh_config(config: &str) -> (tempfile::TempDir, Environment) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".ssh")).unwrap();
    std::fs::write(dir.path().join(".ssh/config"), config).unwrap();
    let env = Environment {
        home: Some(dir.path().into()),
        user: Some("alice".into()),
        path: Some(Path::new("/nonexistent").as_os_str().to_owned()),
    };
    (dir, env)
}

fn shell_args(target: &Target, env: &Environment) -> Vec<String> {
    let invocation = Sftp.shell_command(target, env).unwrap();
    invocation.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
}

const JUMP_CONFIG: &str = "Host box\n  HostName 10.0.0.2\n  Port 2200\n  ProxyJump bastion\n";

#[test]
fn the_shell_for_a_discovered_host_goes_through_its_ssh_config_alias() {
    let (_dir, env) = home_with_ssh_config(JUMP_CONFIG);
    let target = Sftp.discover(&env).unwrap().remove(0);

    assert_eq!(shell_args(&target, &env), ["-o", "HostName=10.0.0.2", "-p", "2200", "-l", "alice", "box"]);
}

#[test]
fn the_shell_through_an_alias_keeps_the_sessions_user_and_port_over_match_blocks() {
    let (_dir, env) = home_with_ssh_config("Host box\n  HostName 10.0.0.2\nMatch all\n  User deploy\n  Port 2222\n");
    let target = Sftp.discover(&env).unwrap().remove(0);

    assert_eq!(shell_args(&target, &env), ["-o", "HostName=10.0.0.2", "-p", "22", "-l", "alice", "box"]);
}

#[test]
fn the_shell_for_a_saved_profile_matching_its_alias_goes_through_the_alias() {
    let (_dir, env) = home_with_ssh_config(JUMP_CONFIG);
    let target = Target {
        name: "box".into(),
        host: "10.0.0.2".into(),
        port: 2200,
        username: "alice".into(),
        password: None,
        options: Default::default(),
    };

    assert_eq!(shell_args(&target, &env), ["-o", "HostName=10.0.0.2", "-p", "2200", "-l", "alice", "box"]);
}

#[test]
fn the_shell_uses_the_explicit_address_when_the_connection_no_longer_matches_its_alias() {
    let (_dir, env) = home_with_ssh_config(JUMP_CONFIG);
    let mut target = Sftp.discover(&env).unwrap().remove(0);
    target.host = "10.0.0.9".into();

    assert_eq!(shell_args(&target, &env), ["-p", "2200", "alice@10.0.0.9"]);
}

#[test]
fn the_shell_uses_the_explicit_address_when_the_identity_file_was_changed() {
    let (dir, env) = home_with_ssh_config("Host box\n  HostName 10.0.0.2\n  IdentityFile ~/.ssh/id\n");
    let mut target = Sftp.discover(&env).unwrap().remove(0);
    let other_key = dir.path().join(".ssh/other").to_string_lossy().into_owned();
    target.options.insert("identity_file".into(), other_key.clone());

    assert_eq!(shell_args(&target, &env), ["-p", "22", "-i", other_key.as_str(), "alice@10.0.0.2"]);
}

#[test]
fn the_shell_uses_the_explicit_address_when_no_ssh_config_names_the_connection() {
    let (_dir, env) = home_with_ssh_config(JUMP_CONFIG);
    let target = Target {
        name: "web".into(),
        host: "10.0.0.2".into(),
        port: 2200,
        username: "alice".into(),
        password: None,
        options: Default::default(),
    };

    assert_eq!(shell_args(&target, &env), ["-p", "2200", "alice@10.0.0.2"]);
}

struct NeverAsked;

#[async_trait::async_trait]
impl Prompter for NeverAsked {
    async fn ask(&mut self, _question: Question) -> Option<Answer> {
        panic!("an unreachable host must fail before any prompt")
    }
}

const PORT_NOTHING_LISTENS_ON: u16 = 1;

#[tokio::test]
async fn connecting_to_an_unreachable_host_reports_a_connect_error() {
    let target = Target {
        name: "unreachable".into(),
        host: "127.0.0.1".into(),
        port: PORT_NOTHING_LISTENS_ON,
        username: "user".into(),
        password: None,
        options: Default::default(),
    };

    let error = Sftp.connect(&target, &mut NeverAsked).await.err().unwrap();

    assert_eq!(error.kind(), ErrorKind::Connect);
}

#[test]
fn the_sftp_form_offers_an_optional_identity_file() {
    let form = Sftp.connection_form();

    assert_eq!(form.port.default, 22);
    assert_eq!(
        form.option("identity_file"),
        Some(&porthmos_vfs::OptionField {
            key: "identity_file",
            label: "Identity file",
            required: false,
            kind: porthmos_vfs::OptionKind::Text { default: "" },
        })
    );
}

#[test]
fn the_sftp_form_uses_no_reserved_keys() {
    assert!(Sftp.connection_form().reserved_key_collisions().is_empty());
}
