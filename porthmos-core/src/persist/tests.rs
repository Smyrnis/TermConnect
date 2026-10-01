use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use super::*;

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn broken(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".broken");
    PathBuf::from(name)
}

fn number(text: &str) -> Result<u32, std::num::ParseIntError> {
    text.trim().parse()
}

#[test]
fn write_atomic_creates_the_parent_and_a_file_with_the_requested_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("deep/er/file.toml");

    write_atomic(&path, b"hello", 0o600).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    assert_eq!(mode(&path), 0o600);
    assert!(!dir.path().join("deep/er/file.toml.tmp").exists());
}

#[test]
fn write_atomic_replaces_a_file_and_keeps_it_private() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    write_atomic(&path, b"one", 0o600).unwrap();

    write_atomic(&path, b"two", 0o600).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"two");
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn a_leftover_temporary_file_with_a_looser_mode_does_not_leak_its_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    let stale = dir.path().join("file.tmp");
    std::fs::write(&stale, b"old").unwrap();
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o666)).unwrap();

    write_atomic(&path, b"secret", 0o600).unwrap();

    assert_eq!(mode(&path), 0o600);
}

#[test]
fn write_atomic_reports_where_it_failed() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"a file").unwrap();

    let error = write_atomic(&blocker.join("file"), b"x", 0o600).unwrap_err();

    assert!(format!("{error:#}").contains("blocker"), "{error:#}");
}

#[test]
fn a_missing_file_is_missing_and_nothing_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("none");

    let loaded = read_or_set_aside(&path, "counter", number);

    assert!(matches!(loaded, Loaded::Missing));
    assert!(!path.exists());
}

#[test]
fn a_valid_file_is_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "42\n").unwrap();

    let loaded = read_or_set_aside(&path, "counter", number);

    assert!(matches!(loaded, Loaded::Ready(42)));
    assert!(path.exists());
}

#[test]
fn a_file_that_does_not_parse_is_moved_aside_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "not a number").unwrap();

    let loaded = read_or_set_aside(&path, "counter", number);

    let Loaded::SetAside { warning, protected } = loaded else {
        panic!("expected the file to be set aside");
    };
    assert!(!protected);
    assert!(warning.contains("The counter file was unreadable"), "{warning}");
    assert!(warning.contains("file.broken"), "{warning}");
    assert_eq!(std::fs::read(broken(&path)).unwrap(), b"not a number");
    assert!(!path.exists());
}

#[test]
fn a_file_that_is_not_utf8_is_moved_aside() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();

    let loaded = read_or_set_aside(&path, "counter", number);

    assert!(matches!(loaded, Loaded::SetAside { protected: false, .. }));
    assert_eq!(std::fs::read(broken(&path)).unwrap(), vec![0xff, 0xfe, 0x00]);
}

#[test]
fn a_second_broken_file_never_replaces_the_first_set_aside() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "first").unwrap();
    read_or_set_aside(&path, "counter", number);
    std::fs::write(&path, "second").unwrap();

    let loaded = read_or_set_aside(&path, "counter", number);

    let Loaded::SetAside { warning, protected } = loaded else {
        panic!("expected the file to be set aside");
    };
    assert!(!protected);
    assert!(warning.contains("file.broken.1"), "{warning}");
    assert_eq!(std::fs::read(broken(&path)).unwrap(), b"first");
    assert_eq!(std::fs::read(dir.path().join("file.broken.1")).unwrap(), b"second");
    assert!(!path.exists());
}

#[test]
fn many_broken_files_each_get_their_own_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    for index in 0..5 {
        std::fs::write(&path, format!("broken {index}")).unwrap();
        read_or_set_aside(&path, "counter", number);
    }

    assert_eq!(std::fs::read(broken(&path)).unwrap(), b"broken 0");
    for index in 1..5 {
        assert_eq!(
            std::fs::read(dir.path().join(format!("file.broken.{index}"))).unwrap(),
            format!("broken {index}").into_bytes()
        );
    }
}

#[test]
fn the_warning_says_what_was_wrong_with_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "twelve").unwrap();

    let Loaded::SetAside { warning, .. } = read_or_set_aside(&path, "counter", number) else {
        panic!("expected the file to be set aside");
    };

    assert!(warning.contains("invalid digit"), "{warning}");
}

#[test]
fn a_file_that_is_not_utf8_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, [0xff, 0xfe]).unwrap();

    let Loaded::SetAside { warning, .. } = read_or_set_aside(&path, "counter", number) else {
        panic!("expected the file to be set aside");
    };

    assert!(warning.contains("UTF-8"), "{warning}");
}

#[test]
fn a_file_that_cannot_be_read_is_moved_aside_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "7").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        return;
    }

    let loaded = read_or_set_aside(&path, "counter", number);

    let Loaded::SetAside { warning, protected } = loaded else {
        panic!("expected the file to be set aside");
    };
    assert!(!protected);
    assert!(warning.starts_with("Couldn't read counter ("), "{warning}");
    std::fs::set_permissions(broken(&path), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(broken(&path)).unwrap(), b"7");
}

#[test]
fn when_the_file_cannot_be_moved_it_is_protected_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("file");
    std::fs::write(&path, "not a number").unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::File::create(folder.join("probe")).is_ok() {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }

    let loaded = read_or_set_aside(&path, "counter", number);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();

    let Loaded::SetAside { warning, protected } = loaded else {
        panic!("expected the file to be set aside");
    };
    assert!(protected);
    assert!(warning.contains("couldn't be set aside"), "{warning}");
    assert!(warning.contains("changes won't be saved"), "{warning}");
    assert_eq!(std::fs::read(&path).unwrap(), b"not a number");
}

#[test]
fn an_older_version_never_replaces_a_newer_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("versioned");

    write_versioned(&path, b"newer", 0o600, 5).unwrap();
    write_versioned(&path, b"older", 0o600, 3).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"newer");
}

#[test]
fn a_newer_version_replaces_an_older_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("versioned");

    write_versioned(&path, b"older", 0o600, 3).unwrap();
    write_versioned(&path, b"newer", 0o600, 5).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"newer");
}

#[test]
fn versions_are_tracked_per_path() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first");
    let second = dir.path().join("second");

    write_versioned(&first, b"a", 0o600, 9).unwrap();
    write_versioned(&second, b"b", 0o600, 2).unwrap();

    assert_eq!(std::fs::read(&second).unwrap(), b"b");
}

#[test]
fn next_version_only_grows() {
    let first = next_version();
    let second = next_version();

    assert!(second > first);
    assert!(first > 0);
}

#[test]
fn a_failed_write_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::create_dir(&target).unwrap();

    let result = write_atomic(&target, b"data", 0o600);

    assert!(result.is_err());
    assert!(!dir.path().join("target.tmp").exists());
}

#[test]
fn paths_written_without_a_version_are_not_kept_in_the_lock_table() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<PathBuf> = (0..30).map(|index| dir.path().join(format!("file{index}"))).collect();

    for path in &paths {
        write_atomic(path, b"x", 0o600).unwrap();
    }

    assert!(paths.iter().all(|path| !is_tracked(path)));
}

#[test]
fn reserving_a_broken_name_never_hands_out_the_same_name_twice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");

    let first = reserve_broken_name(&path).unwrap();
    let second = reserve_broken_name(&path).unwrap();

    assert_ne!(first, second);
    assert!(first.exists() && second.exists());
}
