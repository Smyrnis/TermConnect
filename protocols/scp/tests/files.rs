use std::{path::Path, sync::Arc};

use porthmos_scp::Scp;
use porthmos_ssh::testing::{self, Options, SshServer};
use porthmos_vfs::{Answer, ErrorKind, FileSystem, Protocol};
use tokio::io::AsyncReadExt;

async fn open() -> (SshServer, Arc<dyn FileSystem>) {
    let server = SshServer::start(Options::default()).await;
    let fs = Scp::default()
        .with_connect_options(server.connect_options())
        .connect(&server.target(), &mut testing::answers(vec![Some(Answer::Confirmed)]))
        .await
        .unwrap();
    (server, fs)
}

fn at(server: &SshServer, name: &str) -> std::path::PathBuf {
    server.root.path().join(name)
}

async fn names(fs: &dyn FileSystem, dir: &Path) -> Vec<(String, bool)> {
    let mut items: Vec<(String, bool)> =
        fs.read_dir(dir).await.unwrap().into_iter().map(|item| (item.name, item.metadata.is_dir())).collect();
    items.sort();
    items
}

#[tokio::test]
async fn read_dir_lists_files_and_folders_with_sizes() {
    let (server, fs) = open().await;
    std::fs::write(at(&server, "a.txt"), b"hello").unwrap();
    std::fs::create_dir(at(&server, "sub")).unwrap();

    let items = fs.read_dir(server.root.path()).await.unwrap();

    let file = items.iter().find(|item| item.name == "a.txt").unwrap();
    assert_eq!((file.path.clone(), file.metadata.size, file.metadata.is_dir()), (at(&server, "a.txt"), 5, false));
    assert!(file.metadata.modified.is_some());
    assert_eq!(
        names(fs.as_ref(), server.root.path()).await,
        vec![("a.txt".to_string(), false), ("sub".to_string(), true)]
    );
    assert_eq!(fs.list(server.root.path()).await.unwrap().len(), 2);
}

#[tokio::test]
async fn awkward_names_round_trip() {
    let (server, fs) = open().await;
    let odd = " it's $HOME `id` -x ü.txt";
    std::fs::write(at(&server, odd), b"odd").unwrap();

    let items = fs.read_dir(server.root.path()).await.unwrap();
    assert_eq!(items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), vec![odd]);
    assert_eq!(fs.stat(&items[0].path).await.unwrap().size, 3);
    let mut data = Vec::new();
    fs.open_read(&items[0].path, 0).await.unwrap().read_to_end(&mut data).await.unwrap();
    assert_eq!(data, b"odd");
    fs.rename(&items[0].path, &at(&server, "-renamed $x")).await.unwrap();
    assert_eq!(std::fs::read(at(&server, "-renamed $x")).unwrap(), b"odd");
}

#[tokio::test]
async fn stat_finds_files_and_folders_and_reports_missing_ones() {
    let (server, fs) = open().await;
    std::fs::write(at(&server, "f"), [0u8; 42]).unwrap();
    std::fs::create_dir(at(&server, "d")).unwrap();

    assert_eq!(fs.stat(&at(&server, "f")).await.unwrap().size, 42);
    assert!(fs.stat(&at(&server, "d")).await.unwrap().is_dir());
    assert_eq!(fs.stat(&at(&server, "nope")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn create_dir_refuses_an_existing_folder_and_a_missing_parent() {
    let (server, fs) = open().await;

    fs.create_dir(&at(&server, "new")).await.unwrap();
    assert!(at(&server, "new").is_dir());

    let existing = fs.create_dir(&at(&server, "new")).await.unwrap_err();
    assert_eq!(existing.to_string(), format!("{} already exists", at(&server, "new").display()));
    assert_eq!(fs.create_dir(&at(&server, "no/such")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn rename_moves_files_folders_and_replaces_a_file() {
    let (server, fs) = open().await;
    std::fs::write(at(&server, "a"), b"new").unwrap();
    std::fs::write(at(&server, "b"), b"old").unwrap();
    std::fs::create_dir(at(&server, "d")).unwrap();
    std::fs::write(at(&server, "d/x"), b"x").unwrap();

    fs.rename(&at(&server, "a"), &at(&server, "b")).await.unwrap();
    fs.rename(&at(&server, "d"), &at(&server, "e")).await.unwrap();

    assert_eq!(std::fs::read(at(&server, "b")).unwrap(), b"new");
    assert!(!at(&server, "a").exists());
    assert_eq!(std::fs::read(at(&server, "e/x")).unwrap(), b"x");
}

#[tokio::test]
async fn renames_never_move_into_an_existing_folder() {
    let (server, fs) = open().await;
    std::fs::write(at(&server, "file"), b"f").unwrap();
    std::fs::create_dir(at(&server, "folder")).unwrap();
    std::fs::create_dir(at(&server, "other")).unwrap();

    for from in ["file", "other"] {
        let error = fs.rename(&at(&server, from), &at(&server, "folder")).await.unwrap_err();

        assert_eq!(error.to_string(), format!("{} already exists", at(&server, "folder").display()));
    }
    assert!(std::fs::read_dir(at(&server, "folder")).unwrap().next().is_none());
    assert!(at(&server, "file").exists() && at(&server, "other").is_dir());
}

#[tokio::test]
async fn remove_file_and_delete_a_tree() {
    let (server, fs) = open().await;
    std::fs::write(at(&server, "f"), b"f").unwrap();
    std::fs::create_dir_all(at(&server, "tree/inner")).unwrap();
    std::fs::write(at(&server, "tree/inner/leaf"), b"l").unwrap();

    fs.remove_file(&at(&server, "f")).await.unwrap();
    fs.delete(&at(&server, "tree")).await.unwrap();

    assert!(!at(&server, "f").exists() && !at(&server, "tree").exists());
    assert_eq!(fs.remove_file(&at(&server, "f")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn a_rename_into_a_missing_folder_names_the_target() {
    let (server, fs) = open().await;
    std::fs::write(at(&server, "f"), b"f").unwrap();

    let error = fs.rename(&at(&server, "f"), &at(&server, "no/such/f")).await.unwrap_err();

    assert_eq!(
        (error.kind(), error.to_string()),
        (ErrorKind::NotFound, format!("{} not found", at(&server, "no/such/f").display()))
    );
}
