use std::fs::File;

use super::*;

#[test]
fn list_returns_every_entry_in_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    File::create(dir.path().join("b_file.txt")).unwrap();
    File::create(dir.path().join("a_file.txt")).unwrap();
    fs::create_dir(dir.path().join("z_dir")).unwrap();

    let entries = list(dir.path()).unwrap();
    let mut names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    names.sort();

    assert_eq!(names, vec!["a_file.txt", "b_file.txt", "z_dir"]);
}

#[test]
fn list_reports_unix_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.txt");
    fs::write(&path, b"hello").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

    let entries = list(dir.path()).unwrap();

    assert_eq!(entries[0].permissions.unwrap() & 0o777, 0o640);
}

#[test]
fn list_reports_file_size() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("data.txt"), b"hello").unwrap();

    let entries = list(dir.path()).unwrap();

    assert_eq!(entries[0].size, 5);
}

#[test]
fn create_directory_creates_a_new_directory() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("new_dir");

    create_directory(&target).unwrap();

    assert!(target.is_dir());
}

#[test]
fn rename_moves_a_file_to_a_new_name() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("old.txt");
    let renamed = dir.path().join("new.txt");
    fs::write(&original, b"content").unwrap();

    rename(&original, &renamed).unwrap();

    assert!(!original.exists());
    assert!(renamed.exists());
}

#[test]
fn delete_removes_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("doomed.txt");
    fs::write(&file, b"content").unwrap();

    delete(&file).unwrap();

    assert!(!file.exists());
}

#[test]
fn delete_removes_a_directory_and_its_contents() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("doomed_dir");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("inner.txt"), b"content").unwrap();

    delete(&target).unwrap();

    assert!(!target.exists());
}
