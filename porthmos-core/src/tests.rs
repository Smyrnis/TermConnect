use std::path::Path;

use super::*;

#[cfg(all(feature = "sftp", feature = "ftp", feature = "webdav", feature = "s3", feature = "scp"))]
#[test]
fn builtin_protocols_offer_every_protocol_in_order() {
    let protocols = builtin_protocols(&Paths::in_dir(Path::new("/tmp/t")));

    assert_eq!(
        protocols.iter().map(|protocol| protocol.id()).collect::<Vec<_>>(),
        vec!["sftp", "ftp", "webdav", "s3", "scp"]
    );
}

#[cfg(not(any(feature = "sftp", feature = "ftp", feature = "webdav", feature = "s3", feature = "scp")))]
#[test]
fn without_protocol_features_no_protocol_is_built_in() {
    assert!(builtin_protocols(&Paths::in_dir(Path::new("/tmp/t"))).is_empty());
}

#[test]
fn core_dumps_can_be_switched_off() {
    disable_core_dumps();

    let mut limit = libc::rlimit { rlim_cur: 1, rlim_max: 1 };
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut limit) }, 0);
    assert_eq!(limit.rlim_cur, 0);
}
