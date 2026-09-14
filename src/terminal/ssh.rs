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
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_entry() -> ConnectionEntry {
        ConnectionEntry {
            name: "production".to_string(),
            host: "server.example.com".to_string(),
            port: 2222,
            username: "deploy".to_string(),
            identity_file: Some(PathBuf::from("/home/user/.ssh/id_ed25519")),
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
}
