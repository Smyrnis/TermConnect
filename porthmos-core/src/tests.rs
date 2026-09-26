use std::path::Path;

use super::*;

#[cfg(all(feature = "sftp", feature = "ftp", feature = "webdav"))]
#[test]
fn builtin_protocols_offer_sftp_ftp_and_webdav_in_order() {
    let protocols = builtin_protocols(&Paths::in_dir(Path::new("/tmp/t")));

    assert_eq!(protocols.iter().map(|protocol| protocol.id()).collect::<Vec<_>>(), vec!["sftp", "ftp", "webdav"]);
}

#[cfg(not(any(feature = "sftp", feature = "ftp", feature = "webdav")))]
#[test]
fn without_protocol_features_no_protocol_is_built_in() {
    assert!(builtin_protocols(&Paths::in_dir(Path::new("/tmp/t"))).is_empty());
}
