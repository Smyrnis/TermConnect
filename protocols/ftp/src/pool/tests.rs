use suppaftp::types::Response;

use super::*;

fn reply(status: Status) -> FtpError {
    FtpError::UnexpectedResponse(Response::new(status, b"bye".to_vec()))
}

#[test]
fn a_closing_or_timeout_reply_means_the_connection_is_gone() {
    assert!(is_connection_lost(&reply(Status::Closing)));
    assert!(is_connection_lost(&reply(Status::NotAvailable)));
    assert!(is_connection_lost(&FtpError::BadResponse));
}

#[test]
fn an_ordinary_error_reply_keeps_the_connection() {
    assert!(!is_connection_lost(&reply(Status::FileUnavailable)));
}

#[test]
fn a_failed_data_channel_handshake_means_the_control_connection_is_out_of_step() {
    assert!(is_connection_lost(&FtpError::SecureError("handshake failed".to_string())));
}

#[test]
fn a_timeout_means_the_connection_is_gone() {
    assert!(is_connection_lost(&timed_out()));
}

#[test]
fn a_command_the_server_does_not_implement_is_recognised() {
    assert!(is_not_implemented(&reply(Status::NotImplemented)));
    assert!(is_not_implemented(&reply(Status::BadCommand)));
    assert!(!is_not_implemented(&reply(Status::FileUnavailable)));
}
