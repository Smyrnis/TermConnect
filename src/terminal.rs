use std::process::{Command, ExitStatus};

use crate::connection::ConnectionEntry;

/// Builds the system `ssh` invocation for a connection, using explicit
/// `-p`/`-i`/`user@host` arguments rather than relying on the connection's
/// display name being a valid `~/.ssh/config` alias — it works the same
/// whether the entry came from a saved profile or from parsed SSH config.
pub fn command_for(entry: &ConnectionEntry) -> Command {
    let mut command = Command::new("ssh");
    command.arg("-p").arg(entry.port.to_string());

    if let Some(identity_file) = &entry.identity_file {
        command.arg("-i").arg(identity_file);
    }

    command.arg(format!("{}@{}", entry.username, entry.host));
    command
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
