use std::fs::File;

use porthmos_vfs::{ErrorKind, FileKind};

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

#[tokio::test]
async fn read_dir_reports_symlinks_without_following_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("alias")).unwrap();
    let items = LocalFs::new(dir.path().into()).read_dir(dir.path()).await.unwrap();
    let alias = items.iter().find(|item| item.name == "alias").unwrap();
    assert_eq!(alias.metadata.kind, FileKind::Symlink);
}

#[tokio::test]
async fn stat_follows_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f"), b"12345").unwrap();
    std::os::unix::fs::symlink(dir.path().join("f"), dir.path().join("l")).unwrap();
    let metadata = LocalFs::new(dir.path().into()).stat(&dir.path().join("l")).await.unwrap();
    assert_eq!((metadata.kind, metadata.size), (FileKind::File, 5));
}

#[tokio::test]
async fn open_write_at_an_offset_trims_and_appends() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    std::fs::write(&part, b"abcdef").unwrap();
    let mut writer = LocalFs::new(dir.path().into()).open_write(&part, 3).await.unwrap();
    assert_eq!(writer.offset, 3);
    tokio::io::AsyncWriteExt::write_all(&mut writer.stream, b"XY").await.unwrap();
    tokio::io::AsyncWriteExt::shutdown(&mut writer.stream).await.unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), b"abcXY");
}

#[tokio::test]
async fn open_write_at_zero_truncates() {
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("a.bin.part");
    std::fs::write(&part, b"stale").unwrap();
    drop(LocalFs::new(dir.path().into()).open_write(&part, 0).await.unwrap());
    assert!(std::fs::read(&part).unwrap().is_empty());
}

#[tokio::test]
async fn open_read_starts_at_the_offset() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f"), b"abcdef").unwrap();
    let mut reader = LocalFs::new(dir.path().into()).open_read(&dir.path().join("f"), 2).await.unwrap();
    let mut read = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut read).await.unwrap();
    assert_eq!(read, b"cdef");
}

#[tokio::test]
async fn rename_replaces_an_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a"), b"new").unwrap();
    std::fs::write(dir.path().join("b"), b"old").unwrap();
    LocalFs::new(dir.path().into()).rename(&dir.path().join("a"), &dir.path().join("b")).await.unwrap();
    assert_eq!(std::fs::read(dir.path().join("b")).unwrap(), b"new");
}

#[tokio::test]
async fn local_writes_need_no_resume_backoff() {
    assert_eq!(LocalFs::new("/".into()).resume_backoff(), 0);
}

#[tokio::test]
async fn a_missing_directory_is_not_found() {
    let error = LocalFs::new("/".into()).list(std::path::Path::new("/definitely/missing")).await.unwrap_err();
    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn home_is_the_configured_directory() {
    assert_eq!(LocalFs::new("/srv".into()).home().await.unwrap(), std::path::PathBuf::from("/srv"));
}
