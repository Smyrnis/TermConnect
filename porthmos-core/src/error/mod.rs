use std::fmt::Display;

use porthmos_vfs::{ErrorKind, ProtocolError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

pub fn user_message(context: impl AsRef<str>, err: &dyn Display) -> String {
    format!("{}:\n{err}", context.as_ref())
}

pub fn connect_failure_message(error: &ProtocolError, name: &str, protocol_display: &str) -> String {
    match error.kind() {
        ErrorKind::Auth => user_message(format!("Authentication error for {name}"), error),
        ErrorKind::AuthRejected => format!("Authentication failed for {name}"),
        ErrorKind::SessionStart => {
            user_message(format!("Connected to {name} but failed to start {protocol_display}"), error)
        }
        ErrorKind::Cancelled => "Connection cancelled".to_string(),
        _ => user_message(format!("Unable to connect to {name}"), error),
    }
}

#[cfg(test)]
mod tests;
