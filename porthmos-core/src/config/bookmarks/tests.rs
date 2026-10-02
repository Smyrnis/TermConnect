use std::fs;

use super::*;

fn sample() -> Bookmark {
    Bookmark { label: "projects".to_string(), path: PathBuf::from("/home/user/projects"), host: None }
}

#[test]
fn add_and_remove_manage_the_list() {
    let mut bookmarks = Bookmarks::default();
    bookmarks.add(sample());
    assert_eq!(bookmarks.len(), 1);

    let removed = bookmarks.remove(0).unwrap();
    assert_eq!(removed.label, "projects");
    assert!(bookmarks.is_empty());
}

#[test]
fn load_from_a_missing_file_returns_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bookmarks.toml");

    let (bookmarks, warnings) = load_from(&path).unwrap();

    assert!(bookmarks.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn save_then_load_round_trips_including_a_remote_bookmark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bookmarks.toml");
    let mut bookmarks = Bookmarks::default();
    bookmarks.add(sample());
    bookmarks.add(Bookmark {
        label: "nginx conf".to_string(),
        path: PathBuf::from("/etc/nginx"),
        host: Some("production".to_string()),
    });

    save_to(&path, &bookmarks).unwrap();
    let (loaded, warnings) = load_from(&path).unwrap();

    assert!(warnings.is_empty());
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded.get(1).unwrap().host, Some("production".to_string()));
}

#[test]
fn save_to_leaves_the_original_file_untouched_if_the_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bookmarks.toml");
    let mut original = Bookmarks::default();
    original.add(sample());
    save_to(&path, &original).unwrap();

    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(dir.path()).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(dir.path(), perms.clone()).unwrap();

    let mut updated = original.clone();
    updated.add(Bookmark { label: "extra".to_string(), path: PathBuf::from("/tmp"), host: None });
    let result = save_to(&path, &updated);

    perms.set_mode(0o700);
    fs::set_permissions(dir.path(), perms).unwrap();

    assert!(result.is_err());
    let (loaded, _) = load_from(&path).unwrap();
    assert_eq!(loaded.len(), 1);
}

#[test]
fn load_from_recovers_to_empty_on_malformed_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bookmarks.toml");
    fs::write(&path, "not [ valid").unwrap();

    let (bookmarks, warnings) = load_from(&path).unwrap();

    assert!(bookmarks.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].0.contains("bookmarks.toml.broken"), "{}", warnings[0].0);
    assert!(warnings[0].0.contains("line 1"), "{}", warnings[0].0);
    assert_eq!(fs::read_to_string(dir.path().join("bookmarks.toml.broken")).unwrap(), "not [ valid");
}

#[test]
fn saving_after_a_broken_file_was_set_aside_writes_a_fresh_file_and_keeps_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.bookmarks_file(), "garbage [").unwrap();

    let (mut bookmarks, warnings) = load(&paths).unwrap();
    bookmarks.add(Bookmark { label: "home".into(), path: "/home/me".into(), host: None });
    save_to(&paths.bookmarks_file(), &bookmarks).unwrap();

    assert_eq!(warnings.len(), 1);
    assert_eq!(std::fs::read_to_string(paths.bookmarks_file().with_extension("toml.broken")).unwrap(), "garbage [");
    assert_eq!(load(&paths).unwrap().0.len(), 1);
}

#[test]
fn a_protected_bookmarks_file_is_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let paths = crate::Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.bookmarks_file(), "garbage [").unwrap();
    std::fs::set_permissions(&paths.config_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::File::create(paths.config_dir.join("probe")).is_ok() {
        std::fs::set_permissions(&paths.config_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }

    let (mut bookmarks, _) = load(&paths).unwrap();
    std::fs::set_permissions(&paths.config_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    bookmarks.add(Bookmark { label: "home".into(), path: "/home/me".into(), host: None });

    assert!(bookmarks.is_protected());
    assert!(save_to(&paths.bookmarks_file(), &bookmarks).is_err());
    assert_eq!(std::fs::read_to_string(paths.bookmarks_file()).unwrap(), "garbage [");
}

#[test]
fn the_bookmarks_file_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bookmarks.toml");

    save_to(&path, &Bookmarks::default()).unwrap();

    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
}
