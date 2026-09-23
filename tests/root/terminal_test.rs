use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use super::*;
use crate::connection::ConnectionSource;

fn sample_entry() -> ConnectionEntry {
    ConnectionEntry {
        name: "production".to_string(),
        host: "server.example.com".to_string(),
        port: 2222,
        username: "deploy".to_string(),
        identity_file: Some(PathBuf::from("/home/user/.ssh/id_ed25519")),
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }
}

#[test]
fn command_includes_port_identity_and_user_at_host() {
    let command = command_for(&sample_entry());
    let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();

    assert_eq!(command.get_program(), "ssh");
    assert_eq!(args, vec!["-p", "2222", "-i", "/home/user/.ssh/id_ed25519", "deploy@server.example.com",]);
}

#[test]
fn command_omits_identity_flag_when_none_is_set() {
    let mut entry = sample_entry();
    entry.identity_file = None;

    let command = command_for(&entry);
    let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();

    assert_eq!(args, vec!["-p", "2222", "deploy@server.example.com"]);
}

#[test]
fn command_wraps_with_sshpass_when_password_and_sshpass_are_both_present() {
    let mut entry = sample_entry();
    entry.password = Some("hunter2".to_string());

    let command = command_for_with(&entry, Some(PathBuf::from("/usr/bin/sshpass")));
    let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();

    assert_eq!(command.get_program(), "/usr/bin/sshpass");
    assert_eq!(args, vec!["-e", "ssh", "-p", "2222", "-i", "/home/user/.ssh/id_ed25519", "deploy@server.example.com",]);
    let sshpass_env = command.get_envs().find(|(key, _)| *key == "SSHPASS").and_then(|(_, value)| value);
    assert_eq!(sshpass_env, Some(std::ffi::OsStr::new("hunter2")));
}

#[test]
fn command_falls_back_to_plain_ssh_when_sshpass_is_not_found() {
    let mut entry = sample_entry();
    entry.password = Some("hunter2".to_string());

    let command = command_for_with(&entry, None);

    assert_eq!(command.get_program(), "ssh");
    assert!(command.get_envs().all(|(key, _)| key != "SSHPASS"));
}

#[test]
fn command_falls_back_to_plain_ssh_when_there_is_no_password() {
    let command = command_for_with(&sample_entry(), Some(PathBuf::from("/usr/bin/sshpass")));

    assert_eq!(command.get_program(), "ssh");
    assert!(command.get_envs().all(|(key, _)| key != "SSHPASS"));
}

#[test]
fn find_sshpass_in_locates_an_executable_on_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let sshpass_path = dir.path().join("sshpass");
    fs::write(&sshpass_path, b"").unwrap();
    fs::set_permissions(&sshpass_path, fs::Permissions::from_mode(0o755)).unwrap();

    let path_var = dir.path().to_string_lossy().into_owned();

    assert_eq!(find_sshpass_in(&path_var), Some(sshpass_path));
}

#[test]
fn find_sshpass_in_returns_none_when_not_present() {
    let dir = tempfile::tempdir().unwrap();
    let path_var = dir.path().to_string_lossy().into_owned();

    assert_eq!(find_sshpass_in(&path_var), None);
}

#[test]
fn find_sshpass_in_ignores_a_non_executable_file() {
    let dir = tempfile::tempdir().unwrap();
    let sshpass_path = dir.path().join("sshpass");
    fs::write(&sshpass_path, b"").unwrap();
    fs::set_permissions(&sshpass_path, fs::Permissions::from_mode(0o644)).unwrap();

    let path_var = dir.path().to_string_lossy().into_owned();

    assert_eq!(find_sshpass_in(&path_var), None);
}
