use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::Paths;

fn paths() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    (dir, paths)
}

#[test]
fn the_choice_round_trips_and_defaults_to_off() {
    let (_dir, paths) = paths();
    assert!(!load(&paths).state.save_passwords_in_keyring);

    save(&paths, &UiState { save_passwords_in_keyring: true }).unwrap();
    assert!(load(&paths).state.save_passwords_in_keyring);

    save(&paths, &UiState { save_passwords_in_keyring: false }).unwrap();
    assert!(!load(&paths).state.save_passwords_in_keyring);
}

#[test]
fn a_broken_state_file_means_off() {
    let (_dir, paths) = paths();
    std::fs::create_dir_all(&paths.state_dir).unwrap();

    std::fs::write(paths.state_file(), "save_passwords_in_keyring = \"maybe\"").unwrap();
    assert!(!load(&paths).state.save_passwords_in_keyring);

    std::fs::write(paths.state_file(), "not toml at all [").unwrap();
    assert!(!load(&paths).state.save_passwords_in_keyring);
    assert!(paths.state_file().with_extension("toml.broken").exists());
}

#[test]
fn unknown_keys_are_ignored() {
    let (_dir, paths) = paths();
    std::fs::create_dir_all(&paths.state_dir).unwrap();
    std::fs::write(paths.state_file(), "save_passwords_in_keyring = true\nfuture = 1\n").unwrap();

    assert!(load(&paths).state.save_passwords_in_keyring);
}

#[test]
fn the_state_file_is_private_and_holds_only_the_choice() {
    let (_dir, paths) = paths();

    save(&paths, &UiState { save_passwords_in_keyring: true }).unwrap();

    let mode = std::fs::metadata(paths.state_file()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(std::fs::read_to_string(paths.state_file()).unwrap().trim(), "save_passwords_in_keyring = true");
}

#[test]
fn saving_where_the_state_folder_cannot_exist_is_an_error() {
    let (dir, _paths) = paths();
    std::fs::write(dir.path().join("state"), b"a file, not a folder").unwrap();
    let paths = Paths::in_dir(dir.path());

    assert!(save(&paths, &UiState { save_passwords_in_keyring: true }).is_err());
}

#[test]
fn a_broken_state_file_is_moved_aside_with_a_warning_and_never_overwritten_by_the_next_save() {
    let (_dir, paths) = paths();
    std::fs::create_dir_all(&paths.state_dir).unwrap();
    std::fs::write(paths.state_file(), "not toml at all [").unwrap();

    let loaded = load(&paths);

    assert!(!loaded.state.save_passwords_in_keyring);
    assert!(loaded.warning.unwrap().contains("state.toml.broken"));
    assert!(!loaded.protected);
    assert_eq!(std::fs::read_to_string(paths.state_file().with_extension("toml.broken")).unwrap(), "not toml at all [");
    save(&paths, &UiState { save_passwords_in_keyring: true }).unwrap();
    assert_eq!(std::fs::read_to_string(paths.state_file().with_extension("toml.broken")).unwrap(), "not toml at all [");
}

#[test]
fn a_missing_state_file_loads_quietly() {
    let (_dir, paths) = paths();

    let loaded = load(&paths);

    assert!(loaded.warning.is_none());
    assert!(!loaded.protected);
    assert!(!paths.state_file().exists());
}

#[test]
fn a_state_file_that_cannot_be_moved_aside_is_protected() {
    let (_dir, paths) = paths();
    std::fs::create_dir_all(&paths.state_dir).unwrap();
    std::fs::write(paths.state_file(), "not toml at all [").unwrap();
    std::fs::set_permissions(&paths.state_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::File::create(paths.state_dir.join("probe")).is_ok() {
        std::fs::set_permissions(&paths.state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }

    let loaded = load(&paths);
    std::fs::set_permissions(&paths.state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert!(loaded.protected);
    assert_eq!(std::fs::read_to_string(paths.state_file()).unwrap(), "not toml at all [");
}
