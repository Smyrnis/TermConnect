use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use crate::connection::ConnectionEntry;

/// Builds the system `ssh` invocation for a connection, using explicit
/// `-p`/`-i`/`user@host` arguments rather than relying on the connection's
/// display name being a valid `~/.ssh/config` alias — it works the same
/// whether the entry came from a saved profile or from parsed SSH config.
/// Wraps the command with `sshpass` when a password is saved on the profile
/// and `sshpass` is installed, so the terminal handoff doesn't prompt for
/// it a second time; otherwise behaves exactly as a plain `ssh` call would.
pub fn command_for(entry: &ConnectionEntry) -> Command {
    command_for_with(entry, find_sshpass())
}

fn command_for_with(entry: &ConnectionEntry, sshpass: Option<PathBuf>) -> Command {
    let mut command = match (&entry.password, sshpass) {
        (Some(password), Some(sshpass_path)) => {
            let mut command = Command::new(sshpass_path);
            // `-e` reads the password from the `SSHPASS` env var rather
            // than a `-p <password>` CLI flag, so it never shows up in
            // `ps`/`/proc/*/cmdline` output visible to other local users.
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
    std::env::split_paths(path_var).map(|dir| dir.join("sshpass")).find(|candidate| {
        candidate.is_file()
            && candidate.metadata().map(|metadata| metadata.permissions().mode() & 0o111 != 0).unwrap_or(false)
    })
}

/// Runs the system `ssh` client interactively, blocking until it exits.
/// Must be called with the TUI's alternate screen already left (see
/// `tui::restore`) — this function only owns the `ssh` process itself, not
/// the surrounding terminal lifecycle, which is `app`'s responsibility to
/// keep `terminal/` from depending on `tui/`.
pub fn run(entry: &ConnectionEntry) -> std::io::Result<ExitStatus> {
    command_for(entry).status()
}

#[cfg(test)]
#[path = "../tests/root/terminal_test.rs"]
mod tests;
