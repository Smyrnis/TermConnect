use porthmos_vfs::ErrorKind;

use super::*;

#[test]
fn a_550_about_a_missing_file_is_not_found() {
    assert_eq!(reply_error(550, "/x: No such file or directory").kind(), ErrorKind::NotFound);
}

#[test]
fn a_550_or_553_about_permissions_is_permission_denied() {
    assert_eq!(reply_error(550, "Permission denied").kind(), ErrorKind::PermissionDenied);
    assert_eq!(reply_error(553, "Could not create file.").kind(), ErrorKind::PermissionDenied);
}

#[test]
fn other_replies_keep_the_server_text() {
    let error = reply_error(451, "Local error in processing");

    assert_eq!(error.kind(), ErrorKind::Other);
    assert!(error.to_string().contains("451 Local error in processing"));
}

#[test]
fn a_522_about_session_reuse_explains_the_requirement() {
    let error = reply_error(522, "SSL connection failed: session reuse required");

    assert!(error.to_string().contains("the server requires TLS session reuse on data connections"));
}
