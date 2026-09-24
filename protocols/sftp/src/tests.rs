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

    assert_eq!((targets[0].username.as_str(), targets[0].host.as_str()), ("root", ""));
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
