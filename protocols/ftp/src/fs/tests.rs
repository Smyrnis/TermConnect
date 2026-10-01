use suppaftp::types::Response;

use super::*;

#[test]
fn a_replayed_mkdir_or_removal_that_already_happened_counts_as_done() {
    assert!(already_done(SimpleCommand::MakeDir, true));
    assert!(!already_done(SimpleCommand::MakeDir, false));
    assert!(already_done(SimpleCommand::RemoveFile, false));
    assert!(already_done(SimpleCommand::RemoveDir, false));
    assert!(!already_done(SimpleCommand::RemoveFile, true));
}

#[test]
fn mfmt_is_found_among_the_feature_lines_of_a_reply() {
    assert!(lists_mfmt(b"211-Features:\r\n MDTM\r\n mfmt\r\n211 End\r\n"));
    assert!(lists_mfmt(b"211-Features:\r\n MFMT Modify\r\n211 End\r\n"));
    assert!(!lists_mfmt(b"211-Features:\r\n MDTM\r\n MFMTX\r\n211 End\r\n"));
    assert!(!lists_mfmt(b"211 no features\r\n"));
    assert!(!lists_mfmt(b""));
}

fn feature_response() -> Response {
    Response::new(Status::System, b"211-Features:\r\n MFMT\r\n211 End\r\n".to_vec())
}

#[test]
fn a_feature_reply_is_read_whole_whether_or_not_its_status_was_expected() {
    let matched = whole_reply(Ok(feature_response())).unwrap();
    let unmatched = whole_reply(Err(FtpError::UnexpectedResponse(feature_response()))).unwrap();

    assert_eq!((matched.status, unmatched.status), (Status::System, Status::System));
    assert!(lists_mfmt(&unmatched.body));
    assert!(matches!(whole_reply(Err(FtpError::BadResponse)), Err(FtpError::BadResponse)));
}

#[test]
fn the_feature_command_expects_no_status_so_suppaftp_reads_every_line_of_the_reply() {
    assert!(NO_EXPECTED_STATUS_READS_THE_WHOLE_REPLY.is_empty());
}
