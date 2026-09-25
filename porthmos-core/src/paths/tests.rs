use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
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

    assert_eq!(paths.config_dir, PathBuf::from("/xdg/config/porthmos"));
    assert_eq!(paths.state_dir, PathBuf::from("/xdg/state/porthmos"));
}

#[test]
fn home_is_used_when_xdg_is_unset() {
    let paths = Paths::resolve(None, None, Some(Path::new("/home/u"))).unwrap();

    assert_eq!(paths.config_dir, PathBuf::from("/home/u/.config/porthmos"));
    assert_eq!(paths.state_dir, PathBuf::from("/home/u/.local/state/porthmos"));
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
    assert_eq!(paths.known_certificates_file(), PathBuf::from("/tmp/t/config/known_certificates.toml"));
    assert_eq!(paths.log_file(), PathBuf::from("/tmp/t/state/porthmos.log"));
}

fn paths_with_config_home(config_home: &Path) -> Paths {
    Paths::resolve(Some(config_home.as_os_str().to_owned()), Some(config_home.join("state").into_os_string()), None)
        .unwrap()
}

fn saved_connections(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    let file = dir.join("connections.toml");
    fs::write(&file, "[[connections]]\nname = \"work\"\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    file
}

fn mode_of(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn the_old_config_folder_moves_to_the_new_one_with_its_permissions() {
    let home = tempfile::tempdir().unwrap();
    let paths = paths_with_config_home(home.path());
    let old = home.path().join("termconnect");
    saved_connections(&old);

    let outcome = paths.migrate_legacy_config();

    assert_eq!(outcome, ConfigMigration::Moved { from: old.clone(), to: paths.config_dir.clone() });
    assert!(!old.exists());
    assert_eq!(fs::read_to_string(paths.connections_file()).unwrap(), "[[connections]]\nname = \"work\"\n");
    assert_eq!(mode_of(&paths.connections_file()), 0o600);
    assert_eq!(mode_of(&paths.config_dir), 0o700);
}

#[test]
fn the_old_folder_is_found_under_home_when_xdg_is_unset() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::resolve(None, None, Some(home.path())).unwrap();
    let old = home.path().join(".config").join("termconnect");
    saved_connections(&old);

    let outcome = paths.migrate_legacy_config();

    assert_eq!(outcome, ConfigMigration::Moved { from: old, to: home.path().join(".config").join("porthmos") });
    assert!(paths.connections_file().exists());
}

#[test]
fn when_both_folders_exist_both_are_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let paths = paths_with_config_home(home.path());
    let old = home.path().join("termconnect");
    let old_file = saved_connections(&old);
    fs::create_dir_all(&paths.config_dir).unwrap();
    fs::write(paths.connections_file(), "new").unwrap();

    let outcome = paths.migrate_legacy_config();

    assert_eq!(outcome, ConfigMigration::KeptBoth { legacy: old });
    assert!(old_file.exists());
    assert_eq!(fs::read_to_string(paths.connections_file()).unwrap(), "new");
}

#[test]
fn without_an_old_folder_nothing_happens() {
    let home = tempfile::tempdir().unwrap();
    let paths = paths_with_config_home(home.path());

    assert_eq!(paths.migrate_legacy_config(), ConfigMigration::NothingToMigrate);
    assert!(!paths.config_dir.exists());

    fs::create_dir_all(&paths.config_dir).unwrap();
    assert_eq!(paths.migrate_legacy_config(), ConfigMigration::NothingToMigrate);
}

#[test]
fn a_failed_move_is_reported_and_leaves_an_empty_new_config() {
    let home = tempfile::tempdir().unwrap();
    let config_home = home.path().join("config");
    let paths = paths_with_config_home(&config_home);
    let old = config_home.join("termconnect");
    let old_file = saved_connections(&old);
    fs::set_permissions(&config_home, fs::Permissions::from_mode(0o500)).unwrap();
    if fs::create_dir(config_home.join("probe")).is_ok() {
        return;
    }

    let outcome = paths.migrate_legacy_config();
    fs::set_permissions(&config_home, fs::Permissions::from_mode(0o700)).unwrap();

    assert!(matches!(&outcome, ConfigMigration::Failed { from, .. } if *from == old));
    assert!(old_file.exists());
    assert!(!paths.config_dir.exists());
    let (settings, warnings) = crate::config::load(&paths).unwrap();
    assert_eq!(settings, crate::config::Settings::default());
    assert!(warnings.is_empty());
}
