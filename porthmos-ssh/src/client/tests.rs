use std::os::unix::fs::PermissionsExt;

use russh::keys::PublicKey;
use tokio::sync::mpsc;

use super::*;

const SERVER_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA1Wxd2wRqoDKvqvCwWtQYeIkcRgUv03zkN5wnGjAEL9";
const OTHER_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPn1WisTlluVa9LtieLqa3379O+Ljxx+4mH1KiBjXbMx";
const SERVER_FINGERPRINT: &str = "SHA256:Sh/SULaLCH5FHkfQXVuZsA5t2rQ9B719Sf/t8m2+QkI";

fn key(openssh: &str) -> PublicKey {
    PublicKey::from_openssh(openssh).unwrap()
}

fn known_hosts_with(lines: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".ssh/known_hosts");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, lines).unwrap();
    (dir, path)
}

#[test]
fn a_recorded_key_is_known() {
    let (_dir, path) = known_hosts_with(&format!("example.com {SERVER_KEY}\n"));

    assert_eq!(host_key_status(&path, "example.com", 22, &key(SERVER_KEY)).unwrap(), HostKeyStatus::Known);
}

#[test]
fn a_host_missing_from_known_hosts_is_unknown() {
    let (_dir, path) = known_hosts_with(&format!("other.example.com {SERVER_KEY}\n"));

    assert_eq!(host_key_status(&path, "example.com", 22, &key(SERVER_KEY)).unwrap(), HostKeyStatus::Unknown);
}

#[test]
fn a_missing_known_hosts_file_means_unknown() {
    let dir = tempfile::tempdir().unwrap();

    let status = host_key_status(&dir.path().join(".ssh/known_hosts"), "example.com", 22, &key(SERVER_KEY)).unwrap();

    assert_eq!(status, HostKeyStatus::Unknown);
}

#[test]
fn a_different_key_for_a_recorded_host_is_changed() {
    let (_dir, path) = known_hosts_with(&format!("# comment\nexample.com {OTHER_KEY}\n"));

    assert_eq!(
        host_key_status(&path, "example.com", 22, &key(SERVER_KEY)).unwrap(),
        HostKeyStatus::Changed { line: 2 }
    );
}

#[test]
fn the_changed_line_counts_comments_and_blank_lines() {
    let (_dir, path) = known_hosts_with(&format!("# a\n\n# b\nexample.com {OTHER_KEY}\n"));

    assert_eq!(
        host_key_status(&path, "example.com", 22, &key(SERVER_KEY)).unwrap(),
        HostKeyStatus::Changed { line: 4 }
    );
}

#[test]
fn trusting_a_key_makes_it_known_on_a_non_default_port() {
    let (_dir, path) = known_hosts_with("");

    trust_host_key(&path, "example.com", 2222, &key(SERVER_KEY)).unwrap();

    assert_eq!(host_key_status(&path, "example.com", 2222, &key(SERVER_KEY)).unwrap(), HostKeyStatus::Known);
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.lines().any(|line| line.starts_with("[example.com]:2222 ssh-ed25519 ")), "{contents}");
}

#[test]
fn trusting_a_key_creates_a_private_ssh_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".ssh/known_hosts");

    trust_host_key(&path, "example.com", 22, &key(SERVER_KEY)).unwrap();

    let mode = std::fs::metadata(dir.path().join(".ssh")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert_eq!(host_key_status(&path, "example.com", 22, &key(SERVER_KEY)).unwrap(), HostKeyStatus::Known);
}

fn check_for(known_hosts: Option<PathBuf>) -> (HostKeyCheck, mpsc::Receiver<HostKeyQuestion>) {
    let (questions, asked) = mpsc::channel(1);
    (HostKeyCheck::new("example.com", 22, known_hosts, questions), asked)
}

fn answer_with(mut asked: mpsc::Receiver<HostKeyQuestion>, trusted: bool) -> tokio::task::JoinHandle<(String, String)> {
    tokio::spawn(async move {
        let question = asked.recv().await.unwrap();
        question.reply.send(trusted).unwrap();
        (question.key_type, question.fingerprint)
    })
}

#[tokio::test]
async fn a_known_key_is_accepted_without_asking() {
    let (_dir, path) = known_hosts_with(&format!("example.com {SERVER_KEY}\n"));
    let (check, mut asked) = check_for(Some(path));

    assert!(check.verify(&key(SERVER_KEY)).await.unwrap());
    assert!(asked.try_recv().is_err());
}

#[tokio::test]
async fn a_changed_key_is_refused_without_asking() {
    let (_dir, path) = known_hosts_with(&format!("example.com {OTHER_KEY}\n"));
    let (check, mut asked) = check_for(Some(path));

    let error = check.verify(&key(SERVER_KEY)).await.unwrap_err();

    assert!(error.to_string().contains("REMOTE HOST IDENTIFICATION HAS CHANGED"), "{error}");
    assert!(asked.try_recv().is_err());
}

#[tokio::test]
async fn an_unknown_key_asks_with_its_type_and_fingerprint_and_saves_it_when_trusted() {
    let (_dir, path) = known_hosts_with("");
    let (check, asked) = check_for(Some(path.clone()));
    let answering = answer_with(asked, true);

    assert!(check.verify(&key(SERVER_KEY)).await.unwrap());

    let (key_type, fingerprint) = answering.await.unwrap();
    assert_eq!((key_type.as_str(), fingerprint.as_str()), ("ssh-ed25519", SERVER_FINGERPRINT));
    assert_eq!(host_key_status(&path, "example.com", 22, &key(SERVER_KEY)).unwrap(), HostKeyStatus::Known);
}

#[tokio::test]
async fn declining_an_unknown_key_fails_with_host_key_declined_and_saves_nothing() {
    let (_dir, path) = known_hosts_with("");
    let (check, asked) = check_for(Some(path.clone()));
    answer_with(asked, false);

    let error = check.verify(&key(SERVER_KEY)).await.unwrap_err();

    assert!(error.downcast_ref::<HostKeyDeclined>().is_some(), "{error}");
    assert_eq!(host_key_status(&path, "example.com", 22, &key(SERVER_KEY)).unwrap(), HostKeyStatus::Unknown);
}

#[tokio::test]
async fn an_unknown_key_with_nobody_to_ask_is_declined() {
    let (_dir, path) = known_hosts_with("");
    let (check, asked) = check_for(Some(path));
    drop(asked);

    let error = check.verify(&key(SERVER_KEY)).await.unwrap_err();

    assert!(error.downcast_ref::<HostKeyDeclined>().is_some(), "{error}");
}

#[tokio::test]
async fn without_a_known_hosts_location_the_key_is_refused() {
    let (check, mut asked) = check_for(None);

    let error = check.verify(&key(SERVER_KEY)).await.unwrap_err();

    assert!(error.to_string().contains("known_hosts"), "{error}");
    assert!(asked.try_recv().is_err());
}
