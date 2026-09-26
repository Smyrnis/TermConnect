use porthmos_vfs::{ErrorKind, FileKind};
use russh_sftp::protocol::{FileAttributes, Status, StatusCode};

use super::*;

#[test]
fn the_backoff_covers_every_pipelined_write() {
    use crate::subsystem::{SFTP_MAX_CONCURRENT_WRITES, SFTP_MAX_WRITE_PACKET_LEN};

    assert_eq!(SFTP_RESUME_BACKOFF_BYTES, SFTP_MAX_CONCURRENT_WRITES as u64 * SFTP_MAX_WRITE_PACKET_LEN as u64);
}

#[test]
fn sftp_symlink_metadata_is_reported_as_a_symlink() {
    let mut attributes = FileAttributes::empty();
    attributes.permissions = Some(0o120777);
    attributes.size = Some(7);
    let metadata = metadata_from_sftp(&attributes);
    assert_eq!(metadata.kind, FileKind::Symlink);
    assert_eq!(metadata.size, 7);
}

#[test]
fn sftp_directory_metadata_is_reported_as_a_directory_with_its_mtime() {
    let mut attributes = FileAttributes::empty();
    attributes.permissions = Some(0o040755);
    attributes.mtime = Some(42);
    let metadata = metadata_from_sftp(&attributes);
    assert_eq!((metadata.kind, metadata.modified), (FileKind::Dir, Some(42)));
}

fn status(code: StatusCode) -> russh_sftp::client::error::Error {
    russh_sftp::client::error::Error::Status(Status {
        id: 0,
        status_code: code,
        error_message: "boom".to_string(),
        language_tag: "en".to_string(),
    })
}

#[test]
fn sftp_status_codes_map_to_shared_error_kinds() {
    assert_eq!(sftp_error(status(StatusCode::NoSuchFile)).kind(), ErrorKind::NotFound);
    assert_eq!(sftp_error(status(StatusCode::PermissionDenied)).kind(), ErrorKind::PermissionDenied);
    assert_eq!(sftp_error(status(StatusCode::Failure)).kind(), ErrorKind::Other);
}
