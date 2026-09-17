use std::path::PathBuf;

use super::*;
use crate::connection::{ConnectionEntry, ConnectionSource};

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
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert_eq!(command.get_program(), "ssh");
    assert_eq!(
        args,
        vec![
            "-p",
            "2222",
            "-i",
            "/home/user/.ssh/id_ed25519",
            "deploy@server.example.com",
        ]
    );
}

#[test]
fn command_omits_identity_flag_when_none_is_set() {
    let mut entry = sample_entry();
    entry.identity_file = None;

    let command = command_for(&entry);
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert_eq!(args, vec!["-p", "2222", "deploy@server.example.com"]);
}
