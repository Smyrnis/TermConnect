mod support;

use std::{path::Path, sync::Arc, time::Duration};

use porthmos_ftp::Ftp;
use porthmos_vfs::{ErrorKind, FileKind, FileSystem, Protocol};

async fn connect_to(server: &support::Server) -> Arc<dyn FileSystem> {
    let dir = tempfile::tempdir().unwrap();
    Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, support::USER, Some("pw"), "plain"), &mut support::answers(vec![]))
        .await
        .unwrap()
}

async fn names(fs: &Arc<dyn FileSystem>, dir: &str) -> Vec<String> {
    let mut names: Vec<String> = fs.list(Path::new(dir)).await.unwrap().into_iter().map(|entry| entry.name).collect();
    names.sort();
    names
}

#[tokio::test]
async fn lists_files_and_directories_with_sizes() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("a.txt"), b"12345").unwrap();
    std::fs::create_dir(server.root.path().join("photos")).unwrap();
    let fs = connect_to(&server).await;

    let mut entries = fs.list(Path::new("/")).await.unwrap();
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(
        entries.iter().map(|e| (e.name.as_str(), e.is_dir, e.size)).collect::<Vec<_>>(),
        [("a.txt", false, 5), ("photos", true, entries[1].size)]
    );
    assert_eq!(entries[0].path, Path::new("/a.txt"));
}

#[tokio::test]
async fn stat_reports_files_directories_and_missing_paths() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("a.txt"), b"12345").unwrap();
    std::fs::create_dir(server.root.path().join("photos")).unwrap();
    let fs = connect_to(&server).await;

    let file = fs.stat(Path::new("/a.txt")).await.unwrap();
    assert_eq!((file.kind, file.size), (FileKind::File, 5));
    assert_eq!(fs.stat(Path::new("/photos")).await.unwrap().kind, FileKind::Dir);
    assert_eq!(fs.stat(Path::new("/missing")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn creates_directories() {
    let server = support::start(Some("pw"), None).await;
    let fs = connect_to(&server).await;

    fs.create_dir(Path::new("/new")).await.unwrap();

    assert!(server.root.path().join("new").is_dir());
}

#[tokio::test]
async fn renames_a_file() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("a.txt"), b"a").unwrap();
    let fs = connect_to(&server).await;

    fs.rename(Path::new("/a.txt"), Path::new("/b.txt")).await.unwrap();

    assert_eq!(names(&fs, "/").await, ["b.txt"]);
}

#[tokio::test]
async fn renaming_over_an_existing_file_replaces_it() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("new.txt"), b"new").unwrap();
    std::fs::write(server.root.path().join("old.txt"), b"old").unwrap();
    let fs = connect_to(&server).await;

    fs.rename(Path::new("/new.txt"), Path::new("/old.txt")).await.unwrap();

    assert_eq!(names(&fs, "/").await, ["old.txt"]);
    assert_eq!(std::fs::read(server.root.path().join("old.txt")).unwrap(), b"new");
}

#[tokio::test]
async fn removes_a_file() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("a.txt"), b"a").unwrap();
    let fs = connect_to(&server).await;

    fs.remove_file(Path::new("/a.txt")).await.unwrap();

    assert!(names(&fs, "/").await.is_empty());
}

#[tokio::test]
async fn deletes_a_directory_with_nested_contents() {
    let server = support::start(Some("pw"), None).await;
    let nested = server.root.path().join("tree/inner");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("f.txt"), b"f").unwrap();
    std::fs::write(server.root.path().join("tree/g.txt"), b"g").unwrap();
    let fs = connect_to(&server).await;

    fs.delete(Path::new("/tree")).await.unwrap();

    assert!(!server.root.path().join("tree").exists());
}

#[tokio::test]
async fn a_dropped_idle_connection_is_reopened_transparently() {
    let server = support::start_with(support::Options { password: Some("pw"), tls: None, idle_timeout: Some(1) }).await;
    std::fs::write(server.root.path().join("a.txt"), b"a").unwrap();
    let fs = connect_to(&server).await;

    tokio::time::sleep(Duration::from_millis(2_500)).await;

    assert_eq!(names(&fs, "/").await, ["a.txt"]);
}
