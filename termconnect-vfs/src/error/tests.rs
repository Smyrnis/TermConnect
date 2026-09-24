use super::*;

#[test]
fn io_not_found_maps_to_the_not_found_kind() {
    let error = ProtocolError::from(std::io::Error::from(std::io::ErrorKind::NotFound));
    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[test]
fn io_permission_denied_maps_to_the_permission_denied_kind() {
    let error = ProtocolError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
}

#[test]
fn display_is_the_underlying_message_so_user_text_does_not_change() {
    let error = ProtocolError::new(ErrorKind::Other, anyhow::anyhow!("No such file"));
    assert_eq!(error.to_string(), "No such file");
}
