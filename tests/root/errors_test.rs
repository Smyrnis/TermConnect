use super::*;

#[test]
fn user_message_formats_context_and_error_on_separate_lines() {
    let err = anyhow::anyhow!("connection timed out");
    assert_eq!(
        user_message("Unable to connect to production", &err),
        "Unable to connect to production:\nconnection timed out"
    );
}
