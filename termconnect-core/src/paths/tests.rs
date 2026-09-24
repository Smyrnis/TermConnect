use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::*;

#[test]
fn xdg_directories_win_over_home() {
    let paths = Paths::resolve(
        Some(OsString::from("/xdg/config")),
        Some(OsString::from("/xdg/state")),
        Some(Path::new("/home/u")),
    )
    .unwrap();

    assert_eq!(paths.config_dir, PathBuf::from("/xdg/config/termconnect"));
    assert_eq!(paths.state_dir, PathBuf::from("/xdg/state/termconnect"));
}

#[test]
fn home_is_used_when_xdg_is_unset() {
    let paths = Paths::resolve(None, None, Some(Path::new("/home/u"))).unwrap();

    assert_eq!(paths.config_dir, PathBuf::from("/home/u/.config/termconnect"));
    assert_eq!(paths.state_dir, PathBuf::from("/home/u/.local/state/termconnect"));
}

#[test]
fn without_home_or_xdg_resolution_fails_with_the_home_message() {
    let error = Paths::resolve(None, None, None).unwrap_err();

    assert_eq!(error.to_string(), "HOME environment variable is not set");
}

#[test]
fn files_live_in_their_directories() {
    let paths = Paths::in_dir(Path::new("/tmp/t"));

    assert_eq!(paths.config_file(), PathBuf::from("/tmp/t/config/config.toml"));
    assert_eq!(paths.connections_file(), PathBuf::from("/tmp/t/config/connections.toml"));
    assert_eq!(paths.bookmarks_file(), PathBuf::from("/tmp/t/config/bookmarks.toml"));
    assert_eq!(paths.log_file(), PathBuf::from("/tmp/t/state/termconnect.log"));
}
