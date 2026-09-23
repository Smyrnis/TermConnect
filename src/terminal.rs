use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, ExitStatus},
};

use crate::connection::ConnectionEntry;

pub fn command_for(entry: &ConnectionEntry) -> Command {
    command_for_with(entry, find_sshpass())
}

fn command_for_with(entry: &ConnectionEntry, sshpass: Option<PathBuf>) -> Command {
    let mut command = match (&entry.password, sshpass) {
        (Some(password), Some(sshpass_path)) => {
            let mut command = Command::new(sshpass_path);
            command.arg("-e");
            command.env("SSHPASS", password);
            command.arg("ssh");
            command
        }
        _ => Command::new("ssh"),
    };

    command.arg("-p").arg(entry.port.to_string());
    if let Some(identity_file) = &entry.identity_file {
        command.arg("-i").arg(identity_file);
    }
    command.arg(format!("{}@{}", entry.username, entry.host));
    command
}

fn find_sshpass() -> Option<PathBuf> {
    find_sshpass_in(&std::env::var("PATH").unwrap_or_default())
}

fn find_sshpass_in(path_var: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var).map(|dir| dir.join("sshpass")).find(|candidate| candidate.is_file() && candidate.metadata().map(|metadata| metadata.permissions().mode() & 0o111 != 0).unwrap_or(false))
}

pub fn run(entry: &ConnectionEntry) -> std::io::Result<ExitStatus> {
    command_for(entry).status()
}

#[cfg(test)]
#[path = "../tests/root/terminal_test.rs"]
mod tests;
