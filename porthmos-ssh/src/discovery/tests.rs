use std::path::Path;

use super::*;

#[test]
fn an_identity_file_under_the_home_tilde_is_expanded() {
    let home = Path::new("/home/alice");

    assert_eq!(identity_path("~/.ssh/id_ed25519", Some(home)), Path::new("/home/alice/.ssh/id_ed25519"));
    assert_eq!(identity_path("~", Some(home)), Path::new("/home/alice"));
}

#[test]
fn other_identity_file_paths_are_used_as_written() {
    let home = Path::new("/home/alice");

    assert_eq!(identity_path("/keys/id", Some(home)), Path::new("/keys/id"));
    assert_eq!(identity_path("~bob/id", Some(home)), Path::new("~bob/id"));
    assert_eq!(identity_path("~/.ssh/id", None), Path::new("~/.ssh/id"));
}
