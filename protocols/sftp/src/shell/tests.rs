use std::{collections::BTreeMap, ffi::OsString, fs, os::unix::fs::PermissionsExt, path::PathBuf};

use super::*;

fn sample_target() -> Target {
    let mut options = BTreeMap::new();
    options.insert("identity_file".to_string(), "/home/user/.ssh/id_ed25519".to_string());
    Target {
        name: "production".to_string(),
        host: "server.example.com".to_string(),
        port: 2222,
        username: "deploy".to_string(),
        password: None,
        options,
    }
}

fn args_of(invocation: &ShellInvocation) -> Vec<String> {
    invocation.args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
}

#[test]
fn command_includes_port_identity_and_user_at_host() {
    let invocation = invocation_for(&sample_target(), None);

    assert_eq!(invocation.program, "ssh");
    assert_eq!(
        args_of(&invocation),
        vec!["-p", "2222", "-i", "/home/user/.ssh/id_ed25519", "deploy@server.example.com",]
    );
}

#[test]
fn command_omits_identity_flag_when_none_is_set() {
    let mut target = sample_target();
    target.options.clear();

    let invocation = invocation_for(&target, None);

    assert_eq!(args_of(&invocation), vec!["-p", "2222", "deploy@server.example.com"]);
}

#[test]
fn command_wraps_with_sshpass_when_password_and_sshpass_are_both_present() {
    let mut target = sample_target();
    target.password = Some("hunter2".to_string());

    let invocation = invocation_for(&target, Some(PathBuf::from("/usr/bin/sshpass")));

    assert_eq!(invocation.program, "/usr/bin/sshpass");
    assert_eq!(
        args_of(&invocation),
        vec!["-e", "ssh", "-p", "2222", "-i", "/home/user/.ssh/id_ed25519", "deploy@server.example.com",]
    );
    assert_eq!(invocation.env, vec![(OsString::from("SSHPASS"), OsString::from("hunter2"))]);
}

#[test]
fn command_falls_back_to_plain_ssh_when_sshpass_is_not_found() {
    let mut target = sample_target();
    target.password = Some("hunter2".to_string());

    let invocation = invocation_for(&target, None);

    assert_eq!(invocation.program, "ssh");
    assert!(invocation.env.is_empty());
}

#[test]
fn command_falls_back_to_plain_ssh_when_there_is_no_password() {
    let invocation = invocation_for(&sample_target(), Some(PathBuf::from("/usr/bin/sshpass")));

    assert_eq!(invocation.program, "ssh");
    assert!(invocation.env.is_empty());
}

#[test]
fn find_sshpass_in_locates_an_executable_on_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let sshpass_path = dir.path().join("sshpass");
    fs::write(&sshpass_path, b"").unwrap();
    fs::set_permissions(&sshpass_path, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(find_sshpass_in(dir.path().as_os_str()), Some(sshpass_path));
}

#[test]
fn find_sshpass_in_returns_none_when_not_present() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(find_sshpass_in(dir.path().as_os_str()), None);
}

#[test]
fn find_sshpass_in_ignores_a_non_executable_file() {
    let dir = tempfile::tempdir().unwrap();
    let sshpass_path = dir.path().join("sshpass");
    fs::write(&sshpass_path, b"").unwrap();
    fs::set_permissions(&sshpass_path, fs::Permissions::from_mode(0o644)).unwrap();

    assert_eq!(find_sshpass_in(dir.path().as_os_str()), None);
}
