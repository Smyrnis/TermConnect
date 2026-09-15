use anyhow::Error;

/// Formats an error for the notification queue: the roadmap's two-line
/// shape (`"Unable to connect to production:\nConnection timed out."`).
/// Uses only the error's top-level `Display` message — its full cause
/// chain (`Debug`) stays out of the UI and belongs in the trace log
/// instead; callers pair this with `tracing::debug!("{err:?}")`.
pub fn user_message(context: impl AsRef<str>, err: &Error) -> String {
    format!("{}:\n{err}", context.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_message_formats_context_and_error_on_separate_lines() {
        let err = anyhow::anyhow!("connection timed out");
        assert_eq!(
            user_message("Unable to connect to production", &err),
            "Unable to connect to production:\nconnection timed out"
        );
    }
}
