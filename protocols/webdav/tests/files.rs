mod support;

use std::{path::Path, sync::Arc};

use porthmos_vfs::{ErrorKind, FileSystem, Protocol};
use porthmos_webdav::WebDav;
use support::{Options, Server};

async fn open(options: Options) -> (Server, Arc<dyn FileSystem>, tempfile::TempDir) {
    let server = support::start(options).await;
    let dir = tempfile::tempdir().unwrap();
    let target = support::target(server.port, "", None);
    let fs = WebDav::new(dir.path().join("k.toml")).connect(&target, &mut support::answers(vec![])).await.unwrap();
    (server, fs, dir)
}

#[tokio::test]
async fn read_dir_reports_kinds_sizes_and_times_without_the_directory_itself() {
    let (server, fs, _dir) = open(Options::default()).await;
    std::fs::create_dir(server.root.path().join("docs")).unwrap();
    std::fs::write(server.root.path().join("docs/a.txt"), b"hello").unwrap();
    std::fs::create_dir(server.root.path().join("docs/sub")).unwrap();

    let mut items = fs.read_dir(Path::new("/docs")).await.unwrap();
    items.sort_by(|left, right| left.name.cmp(&right.name));

    assert_eq!(items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), vec!["a.txt", "sub"]);
    assert_eq!(items[0].path, Path::new("/docs/a.txt"));
    assert_eq!((items[0].metadata.size, items[0].metadata.is_dir()), (5, false));
    assert!(items[0].metadata.modified.is_some());
    assert!(items[1].metadata.is_dir());
    let entries = fs.list(Path::new("/docs")).await.unwrap();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn stat_finds_files_and_directories_and_reports_missing_ones() {
    let (server, fs, _dir) = open(Options::default()).await;
    std::fs::create_dir(server.root.path().join("docs")).unwrap();
    std::fs::write(server.root.path().join("f.bin"), vec![0u8; 42]).unwrap();

    assert_eq!(fs.stat(Path::new("/f.bin")).await.unwrap().size, 42);
    assert!(fs.stat(Path::new("/docs")).await.unwrap().is_dir());
    assert_eq!(fs.stat(Path::new("/nope")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn create_dir_makes_it_refuses_an_existing_one_and_a_missing_parent() {
    let (server, fs, _dir) = open(Options::default()).await;

    fs.create_dir(Path::new("/new")).await.unwrap();
    assert!(server.root.path().join("new").is_dir());

    let existing = fs.create_dir(Path::new("/new")).await.unwrap_err();
    assert_eq!((existing.kind(), existing.to_string().as_str()), (ErrorKind::Other, "/new already exists"));
    assert_eq!(fs.create_dir(Path::new("/no/such/parent")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn rename_moves_files_and_directories_and_replaces_an_existing_target() {
    let (server, fs, _dir) = open(Options::default()).await;
    let root = server.root.path();
    std::fs::write(root.join("a.txt"), b"new").unwrap();
    std::fs::write(root.join("b.txt"), b"old").unwrap();
    std::fs::create_dir(root.join("d")).unwrap();
    std::fs::write(root.join("d/x"), b"x").unwrap();

    fs.rename(Path::new("/a.txt"), Path::new("/b.txt")).await.unwrap();
    fs.rename(Path::new("/d"), Path::new("/e")).await.unwrap();

    assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"new");
    assert!(!root.join("a.txt").exists());
    assert_eq!(std::fs::read(root.join("e/x")).unwrap(), b"x");
}

#[tokio::test]
async fn remove_file_and_recursive_delete() {
    let (server, fs, _dir) = open(Options::default()).await;
    let root = server.root.path();
    std::fs::write(root.join("f"), b"f").unwrap();
    std::fs::create_dir_all(root.join("tree/inner")).unwrap();
    std::fs::write(root.join("tree/inner/leaf"), b"l").unwrap();

    fs.remove_file(Path::new("/f")).await.unwrap();
    fs.delete(Path::new("/tree")).await.unwrap();

    assert!(!root.join("f").exists());
    assert!(!root.join("tree").exists());
}

#[tokio::test]
async fn awkward_names_round_trip() {
    let (server, fs, _dir) = open(Options::default()).await;
    let name = "a #1 100% & ü.txt";
    std::fs::write(server.root.path().join(name), b"odd").unwrap();

    let items = fs.read_dir(Path::new("/")).await.unwrap();
    assert_eq!(items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), vec![name]);
    assert_eq!(fs.stat(&items[0].path).await.unwrap().size, 3);

    fs.rename(&items[0].path, Path::new("/renamed ü & #2.txt")).await.unwrap();
    assert_eq!(std::fs::read(server.root.path().join("renamed ü & #2.txt")).unwrap(), b"odd");
}

#[tokio::test]
async fn a_root_typed_without_slashes_lists_the_share() {
    let server = support::start(Options { prefix: "/dav", ..Options::default() }).await;
    std::fs::write(server.root.path().join("inside.txt"), b"in").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = support::with_option(support::target(server.port, "", None), "root", "dav");

    let fs = WebDav::new(dir.path().join("k.toml")).connect(&target, &mut support::answers(vec![])).await.unwrap();

    let items = fs.read_dir(Path::new("/")).await.unwrap();
    assert_eq!(items.iter().map(|item| item.path.as_path()).collect::<Vec<_>>(), vec![Path::new("/inside.txt")]);
}

#[tokio::test]
async fn renaming_a_directory_onto_an_existing_one_is_refused_and_keeps_it() {
    let (server, fs, _dir) = open(Options::default()).await;
    let root = server.root.path();
    std::fs::create_dir(root.join("a")).unwrap();
    std::fs::create_dir(root.join("b")).unwrap();
    std::fs::write(root.join("b/keep"), b"precious").unwrap();

    let error = fs.rename(Path::new("/a"), Path::new("/b")).await.unwrap_err();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Other, "/b already exists"));
    assert_eq!(std::fs::read(root.join("b/keep")).unwrap(), b"precious");
    assert!(root.join("a").is_dir());
}

#[tokio::test]
async fn a_listing_whose_paths_are_outside_the_root_is_an_error() {
    let server =
        support::start(Options { prefix: "/dav", quirk: support::Quirk::ForeignHrefs, ..Options::default() }).await;
    std::fs::write(server.root.path().join("f"), b"f").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = support::with_option(support::target(server.port, "", None), "root", "/dav");
    let fs = WebDav::new(dir.path().join("k.toml")).connect(&target, &mut support::answers(vec![])).await.unwrap();

    let error = fs.read_dir(Path::new("/")).await.unwrap_err();

    assert_eq!(
        (error.kind(), error.to_string().as_str()),
        (ErrorKind::Other, "the server answered with paths outside Root path /dav/")
    );
}

#[tokio::test]
async fn renaming_a_file_onto_an_existing_directory_is_refused_and_keeps_it() {
    let (server, fs, _dir) = open(Options::default()).await;
    let root = server.root.path();
    std::fs::write(root.join("notes"), b"n").unwrap();
    std::fs::create_dir(root.join("photos")).unwrap();
    std::fs::write(root.join("photos/keep"), b"precious").unwrap();

    let error = fs.rename(Path::new("/notes"), Path::new("/photos")).await.unwrap_err();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Other, "/photos already exists"));
    assert_eq!(std::fs::read(root.join("photos/keep")).unwrap(), b"precious");
    assert_eq!(std::fs::read(root.join("notes")).unwrap(), b"n");
}

#[tokio::test]
async fn a_partially_failed_move_is_an_error() {
    let (server, fs, _dir) = open(Options { quirk: support::Quirk::PartialMove, ..Options::default() }).await;
    std::fs::create_dir(server.root.path().join("d")).unwrap();

    let error = fs.rename(Path::new("/d"), Path::new("/e")).await.unwrap_err();

    assert_eq!(
        (error.kind(), error.to_string().as_str()),
        (ErrorKind::Other, "/d: could not move /d/locked.txt (423)")
    );
}

#[tokio::test]
async fn a_move_the_server_never_answers_times_out() {
    let server = support::start(Options { quirk: support::Quirk::HangMove, ..Options::default() }).await;
    std::fs::write(server.root.path().join("f"), b"f").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let fs = WebDav::new(dir.path().join("k.toml"))
        .with_timeout(std::time::Duration::from_millis(50))
        .connect(&support::target(server.port, "", None), &mut support::answers(vec![]))
        .await
        .unwrap();

    let error = fs.rename(Path::new("/f"), Path::new("/g")).await.unwrap_err();

    assert_eq!(error.to_string(), "the server did not respond in time");
}
