mod support;

use std::{path::Path, sync::Arc};

use porthmos_s3::S3;
use porthmos_vfs::{ErrorKind, FileSystem, Protocol};
use support::{BUCKET, Options, Quirk, SECRET, Server};
use tokio::io::AsyncReadExt;

async fn open_with(options: Options, bucket: Option<&str>) -> (Server, Arc<dyn FileSystem>, tempfile::TempDir) {
    let server = support::start(options).await;
    let dir = tempfile::tempdir().unwrap();
    let fs = S3::new(dir.path().join("k.toml"))
        .connect(&support::target(server.port, bucket, Some(SECRET)), &mut support::answers(vec![]))
        .await
        .unwrap();
    (server, fs, dir)
}

async fn open(options: Options) -> (Server, Arc<dyn FileSystem>, tempfile::TempDir) {
    open_with(options, Some(BUCKET)).await
}

async fn names(fs: &dyn FileSystem, dir: &str) -> Vec<(String, bool)> {
    let mut items: Vec<(String, bool)> = fs
        .read_dir(Path::new(dir))
        .await
        .unwrap()
        .into_iter()
        .map(|item| (item.name, item.metadata.is_dir()))
        .collect();
    items.sort();
    items
}

fn put(server: &Server, key: &str, data: &[u8]) {
    let path = server.bucket_dir().join(key);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

#[tokio::test]
async fn read_dir_shows_folders_and_files() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "docs/a.txt", b"hello");
    put(&server, "docs/sub/x", b"x");

    let items = fs.read_dir(Path::new("/docs")).await.unwrap();

    let file = items.iter().find(|item| item.name == "a.txt").unwrap();
    assert_eq!((file.path.as_path(), file.metadata.size, file.metadata.is_dir()), (Path::new("/docs/a.txt"), 5, false));
    assert!(file.metadata.modified.is_some());
    assert_eq!(names(fs.as_ref(), "/docs").await, vec![("a.txt".to_string(), false), ("sub".to_string(), true)]);
    assert_eq!(fs.list(Path::new("/docs")).await.unwrap().len(), 2);
}

#[tokio::test]
async fn a_new_folder_is_listed_and_its_marker_is_hidden() {
    let (_server, fs, _dir) = open(Options::default()).await;

    fs.create_dir(Path::new("/empty")).await.unwrap();

    assert_eq!(names(fs.as_ref(), "/").await, vec![("empty".to_string(), true)]);
    assert!(names(fs.as_ref(), "/empty").await.is_empty());
    assert!(fs.stat(Path::new("/empty")).await.unwrap().is_dir());
}

#[tokio::test]
async fn listing_follows_continuation_pages() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::PageSize(2), ..Options::default() }).await;
    for index in 0..5 {
        put(&server, &format!("f{index}"), b"x");
    }

    assert_eq!(fs.read_dir(Path::new("/")).await.unwrap().len(), 5);
}

#[tokio::test]
async fn stat_finds_files_and_folders_and_reports_missing_ones() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "docs/a.txt", &[0u8; 42]);

    assert_eq!(fs.stat(Path::new("/docs/a.txt")).await.unwrap().size, 42);
    assert!(fs.stat(Path::new("/docs")).await.unwrap().is_dir());
    assert!(fs.stat(Path::new("/")).await.unwrap().is_dir());
    assert_eq!(fs.stat(Path::new("/nope")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn awkward_keys_round_trip() {
    let (server, fs, _dir) = open(Options::default()).await;
    let name = "a #1 100% & ü +x.txt";
    put(&server, name, b"odd");

    let items = fs.read_dir(Path::new("/")).await.unwrap();
    assert_eq!(items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), vec![name]);
    assert_eq!(fs.stat(&items[0].path).await.unwrap().size, 3);
    let mut data = Vec::new();
    fs.open_read(&items[0].path, 0).await.unwrap().read_to_end(&mut data).await.unwrap();
    assert_eq!(data, b"odd");
}

#[tokio::test]
async fn renaming_a_file_moves_it_and_replaces_a_file() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "a.txt", b"new");
    put(&server, "b.txt", b"old");

    fs.rename(Path::new("/a.txt"), Path::new("/b.txt")).await.unwrap();

    assert_eq!(std::fs::read(server.bucket_dir().join("b.txt")).unwrap(), b"new");
    assert!(!server.bucket_dir().join("a.txt").exists());
}

#[tokio::test]
async fn renaming_a_folder_moves_every_key() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "d/x", b"x");
    put(&server, "d/sub/y", b"y");

    fs.rename(Path::new("/d"), Path::new("/e")).await.unwrap();

    assert_eq!(std::fs::read(server.bucket_dir().join("e/x")).unwrap(), b"x");
    assert_eq!(std::fs::read(server.bucket_dir().join("e/sub/y")).unwrap(), b"y");
    assert_eq!(fs.stat(Path::new("/d")).await.unwrap_err().kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn renaming_awkward_keys() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "a #1 & ü.txt", b"odd");

    fs.rename(Path::new("/a #1 & ü.txt"), Path::new("/b %2 + ü.txt")).await.unwrap();

    assert_eq!(std::fs::read(server.bucket_dir().join("b %2 + ü.txt")).unwrap(), b"odd");
}

#[tokio::test]
async fn renames_never_overwrite_folders() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "notes", b"n");
    put(&server, "photos/keep", b"precious");
    put(&server, "d/x", b"x");
    put(&server, "file", b"f");

    for (from, to) in [("/notes", "/photos"), ("/d", "/photos"), ("/d", "/file")] {
        let error = fs.rename(Path::new(from), Path::new(to)).await.unwrap_err();

        assert_eq!((error.kind(), error.to_string()), (ErrorKind::Other, format!("{to} already exists")));
    }
    assert_eq!(std::fs::read(server.bucket_dir().join("photos/keep")).unwrap(), b"precious");
    assert_eq!(std::fs::read(server.bucket_dir().join("notes")).unwrap(), b"n");
    assert_eq!(std::fs::read(server.bucket_dir().join("d/x")).unwrap(), b"x");
}

#[tokio::test]
async fn remove_file_and_delete_a_file() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "a", b"a");
    put(&server, "b", b"b");

    fs.remove_file(Path::new("/a")).await.unwrap();
    fs.delete(Path::new("/b")).await.unwrap();

    assert!(fs.read_dir(Path::new("/")).await.unwrap().is_empty());
}

#[tokio::test]
async fn deleting_a_folder_with_more_than_a_thousand_keys() {
    let (server, fs, _dir) = open(Options::default()).await;
    for index in 0..1_005 {
        put(&server, &format!("big/f{index}"), b"x");
    }
    put(&server, "keep", b"k");

    fs.delete(Path::new("/big")).await.unwrap();

    assert_eq!(names(fs.as_ref(), "/").await, vec![("keep".to_string(), false)]);
}

#[tokio::test]
async fn a_server_without_batch_delete_deletes_one_by_one() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::NoDeleteObjects, ..Options::default() }).await;
    for index in 0..3 {
        put(&server, &format!("d/f{index}"), b"x");
    }

    fs.delete(Path::new("/d")).await.unwrap();

    assert!(names(fs.as_ref(), "/").await.is_empty());
}

#[tokio::test]
async fn buckets_cannot_be_created_removed_or_crossed() {
    let (_server, fs, _dir) = open_with(Options { buckets: &["bkt", "other"], ..Options::default() }, None).await;
    fs.create_dir(Path::new("/bkt/folder")).await.unwrap();

    for error in [
        fs.create_dir(Path::new("/new")).await.unwrap_err(),
        fs.delete(Path::new("/bkt")).await.unwrap_err(),
        fs.rename(Path::new("/bkt"), Path::new("/renamed")).await.unwrap_err(),
    ] {
        assert_eq!(
            (error.kind(), error.to_string().as_str()),
            (ErrorKind::PermissionDenied, "buckets can't be created or removed here")
        );
    }
    let crossing = fs.rename(Path::new("/bkt/folder"), Path::new("/other/folder")).await.unwrap_err();
    assert_eq!(crossing.to_string(), "moving between buckets is not supported");
}

#[tokio::test]
async fn a_key_without_upload_listing_can_still_browse() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::DenyUploadListing, ..Options::default() }).await;
    put(&server, "docs/a.txt", b"a");

    assert_eq!(names(fs.as_ref(), "/docs").await, vec![("a.txt".to_string(), false)]);
    assert_eq!(fs.stat(Path::new("/docs/a.txt")).await.unwrap().size, 1);
}

#[tokio::test]
async fn buckets_in_another_region_open_in_bucket_list_mode() {
    let (server, fs, _dir) = open_with(
        Options { quirk: Quirk::BucketRegion("eu-bucket", "eu-west-1"), buckets: &["bkt", "eu-bucket"] },
        None,
    )
    .await;
    std::fs::write(server.root.path().join("eu-bucket/far.txt"), b"far").unwrap();
    std::fs::write(server.root.path().join("bkt/near.txt"), b"near").unwrap();

    assert_eq!(names(fs.as_ref(), "/eu-bucket").await, vec![("far.txt".to_string(), false)]);
    assert_eq!(names(fs.as_ref(), "/bkt").await, vec![("near.txt".to_string(), false)]);
    let mut data = Vec::new();
    fs.open_read(Path::new("/eu-bucket/far.txt"), 0).await.unwrap().read_to_end(&mut data).await.unwrap();
    assert_eq!(data, b"far");
}

#[tokio::test]
async fn a_copy_that_fails_inside_a_success_reply_keeps_the_source() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::CopyErrorInSuccess, ..Options::default() }).await;
    put(&server, "a.txt", b"keep me");

    assert!(fs.rename(Path::new("/a.txt"), Path::new("/b.txt")).await.is_err());

    assert_eq!(std::fs::read(server.bucket_dir().join("a.txt")).unwrap(), b"keep me");
    assert!(!server.bucket_dir().join("b.txt").exists());
}

#[tokio::test]
async fn renaming_a_file_onto_itself_keeps_it() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "same.txt", b"same");

    fs.rename(Path::new("/same.txt"), Path::new("/same.txt")).await.unwrap();

    assert_eq!(std::fs::read(server.bucket_dir().join("same.txt")).unwrap(), b"same");
}

#[tokio::test]
async fn renaming_a_folder_abandons_its_unfinished_uploads() {
    let (server, fs, _dir) = open(Options::default()).await;
    put(&server, "d/y", b"y");
    let mut writer = fs.open_write(Path::new("/d/x.part"), 0).await.unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut writer.stream, b"partial").await.unwrap();
    tokio::io::AsyncWriteExt::shutdown(&mut writer.stream).await.unwrap();
    assert_eq!(server.registry.pending().len(), 1);

    fs.rename(Path::new("/d"), Path::new("/e")).await.unwrap();

    assert!(server.registry.pending().is_empty());
    assert_eq!(std::fs::read(server.bucket_dir().join("e/y")).unwrap(), b"y");
}

#[tokio::test]
async fn a_short_copy_keeps_the_source_and_removes_the_bad_copy() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::ShortCopy, ..Options::default() }).await;
    put(&server, "a.txt", b"0123456789");

    let error = fs.rename(Path::new("/a.txt"), Path::new("/b.txt")).await.unwrap_err();

    assert_eq!(error.to_string(), "the copy of /a.txt is incomplete");
    assert_eq!(std::fs::read(server.bucket_dir().join("a.txt")).unwrap(), b"0123456789");
    assert!(!server.bucket_dir().join("b.txt").exists());
}
