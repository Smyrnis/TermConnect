mod support;

use std::{path::Path, sync::Arc};

use porthmos_vfs::{FileSystem, Protocol};
use porthmos_webdav::WebDav;
use support::{Auth, Options, Quirk, Server, USER};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn open(options: Options) -> (Server, Arc<dyn FileSystem>, tempfile::TempDir) {
    let server = support::start(options).await;
    let dir = tempfile::tempdir().unwrap();
    let (username, password) = match options.auth {
        Auth::Open => ("", None),
        Auth::Basic(password) | Auth::BasicForWrites(password) | Auth::Digest(password) => (USER, Some(password)),
    };
    let target = support::target(server.port, username, password);
    let fs = WebDav::new(dir.path().join("k.toml")).connect(&target, &mut support::answers(vec![])).await.unwrap();
    (server, fs, dir)
}

async fn read_all(fs: &dyn FileSystem, path: &str, offset: u64) -> Vec<u8> {
    let mut reader = fs.open_read(Path::new(path), offset).await.unwrap();
    let mut data = Vec::new();
    reader.read_to_end(&mut data).await.unwrap();
    data
}

async fn write_all(fs: &dyn FileSystem, path: &str, offset: u64, data: &[u8]) -> std::io::Result<u64> {
    let mut writer = fs.open_write(Path::new(path), offset).await.map_err(std::io::Error::other)?;
    writer.stream.write_all(data).await?;
    writer.stream.shutdown().await?;
    Ok(writer.offset)
}

#[tokio::test]
async fn reads_whole_files_and_from_an_offset() {
    let (server, fs, _dir) = open(Options::default()).await;
    std::fs::write(server.root.path().join("f"), b"hello world").unwrap();

    assert_eq!(read_all(fs.as_ref(), "/f", 0).await, b"hello world");
    assert_eq!(read_all(fs.as_ref(), "/f", 6).await, b"world");
}

#[tokio::test]
async fn a_resumed_read_against_a_server_ignoring_range_starts_at_the_offset() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::IgnoreRange, ..Options::default() }).await;
    std::fs::write(server.root.path().join("f"), b"hello world").unwrap();

    assert_eq!(read_all(fs.as_ref(), "/f", 6).await, b"world");
}

#[tokio::test]
async fn writes_stream_a_new_file() {
    let (server, fs, _dir) = open(Options::default()).await;
    let data: Vec<u8> = (0..3_000_000u32).map(|index| (index % 251) as u8).collect();

    assert_eq!(write_all(fs.as_ref(), "/big.bin", 0, &data).await.unwrap(), 0);
    assert_eq!(std::fs::read(server.root.path().join("big.bin")).unwrap(), data);
}

#[tokio::test]
async fn a_resumed_write_appends_in_chunks_when_partial_updates_are_supported() {
    let (server, fs, _dir) = open(Options::default()).await;
    let head = b"0123456789".to_vec();
    let tail: Vec<u8> = (0..5_000_000u32).map(|index| (index % 253) as u8).collect();
    std::fs::write(server.root.path().join("f.part"), &head).unwrap();

    assert_eq!(write_all(fs.as_ref(), "/f.part", 10, &tail).await.unwrap(), 10);

    let stored = std::fs::read(server.root.path().join("f.part")).unwrap();
    assert_eq!(stored.len(), head.len() + tail.len());
    assert_eq!(&stored[..10], head.as_slice());
    assert_eq!(&stored[10..], tail.as_slice());
}

#[tokio::test]
async fn a_resumed_write_restarts_from_zero_without_partial_updates() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::NoPartialUpdate, ..Options::default() }).await;
    std::fs::write(server.root.path().join("f.part"), b"stale").unwrap();

    let mut writer = fs.open_write(Path::new("/f.part"), 5).await.unwrap();
    assert_eq!(writer.offset, 0);
    writer.stream.write_all(b"fresh content").await.unwrap();
    writer.stream.shutdown().await.unwrap();

    assert_eq!(std::fs::read(server.root.path().join("f.part")).unwrap(), b"fresh content");
}

#[tokio::test]
async fn an_upload_the_server_rejects_fails_at_shutdown() {
    let (_server, fs, _dir) = open(Options { quirk: Quirk::RejectPut(507), ..Options::default() }).await;

    let error = write_all(fs.as_ref(), "/f", 0, b"data").await.unwrap_err();

    assert!(error.to_string().contains("insufficient storage on the server"), "{error}");
}

#[tokio::test]
async fn an_upload_the_server_stores_short_fails_at_shutdown() {
    let (_server, fs, _dir) = open(Options { quirk: Quirk::EmptyPut, ..Options::default() }).await;

    let error = write_all(fs.as_ref(), "/f", 0, b"12345").await.unwrap_err();

    assert!(error.to_string().contains("the server stored 0 of 5 bytes"), "{error}");
}

#[tokio::test]
async fn a_rotating_digest_nonce_is_followed_silently() {
    let (server, fs, _dir) =
        open(Options { auth: Auth::Digest("pw"), quirk: Quirk::RotateNonce, ..Options::default() }).await;
    std::fs::write(server.root.path().join("f"), b"x").unwrap();

    for _ in 0..10 {
        assert_eq!(fs.read_dir(Path::new("/")).await.unwrap().len(), 1);
    }
    assert_eq!(read_all(fs.as_ref(), "/f", 0).await, b"x");
}

#[tokio::test]
async fn parallel_transfers_run_while_listing() {
    let (server, fs, _dir) = open(Options { auth: Auth::Digest("pw"), ..Options::default() }).await;
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..4u8 {
        let fs = fs.clone();
        tasks.spawn(async move {
            let data = vec![index; 1_000_000];
            write_all(fs.as_ref(), &format!("/p{index}"), 0, &data).await.unwrap();
        });
    }
    for _ in 0..4 {
        fs.read_dir(Path::new("/")).await.unwrap();
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    for index in 0..4u8 {
        assert_eq!(std::fs::read(server.root.path().join(format!("p{index}"))).unwrap(), vec![index; 1_000_000]);
    }
}

#[tokio::test]
async fn an_upload_slower_than_the_timeout_completes() {
    let server = support::start(Options::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let fs = WebDav::new(dir.path().join("k.toml"))
        .with_timeout(std::time::Duration::from_millis(300))
        .connect(&support::target(server.port, "", None), &mut support::answers(vec![]))
        .await
        .unwrap();

    let mut writer = fs.open_write(Path::new("/slow"), 0).await.unwrap();
    for _ in 0..4 {
        writer.stream.write_all(&[7u8; 1000]).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    writer.stream.shutdown().await.unwrap();

    assert_eq!(std::fs::read(server.root.path().join("slow")).unwrap(), vec![7u8; 4000]);
}

#[tokio::test]
async fn awkward_names_are_written_and_read_back() {
    let (server, fs, _dir) = open(Options::default()).await;
    let name = "/a #1 100% & ü.txt";

    write_all(fs.as_ref(), name, 0, b"odd bytes").await.unwrap();

    assert_eq!(std::fs::read(server.root.path().join(&name[1..])).unwrap(), b"odd bytes");
    assert_eq!(read_all(fs.as_ref(), name, 4).await, b"bytes");
}

#[tokio::test]
async fn a_put_that_meets_a_stale_nonce_succeeds_when_retried() {
    let (server, fs, _dir) =
        open(Options { auth: Auth::Digest("pw"), quirk: Quirk::RotateNonce, ..Options::default() }).await;

    for index in 0..6 {
        let path = format!("/f{index}");
        if write_all(fs.as_ref(), &path, 0, b"data").await.is_err() {
            write_all(fs.as_ref(), &path, 0, b"data").await.unwrap();
        }
        assert_eq!(std::fs::read(server.root.path().join(&path[1..])).unwrap(), b"data");
    }
}

#[tokio::test]
async fn resumed_chunks_follow_a_rotating_digest_nonce() {
    let (server, fs, _dir) =
        open(Options { auth: Auth::Digest("pw"), quirk: Quirk::RotateNonce, ..Options::default() }).await;
    std::fs::write(server.root.path().join("f.part"), b"head").unwrap();
    let tail: Vec<u8> = (0..13_000_000u32).map(|index| (index % 241) as u8).collect();

    assert_eq!(write_all(fs.as_ref(), "/f.part", 4, &tail).await.unwrap(), 4);

    let stored = std::fs::read(server.root.path().join("f.part")).unwrap();
    assert_eq!((&stored[..4], stored.len()), (b"head".as_slice(), 4 + tail.len()));
}

#[tokio::test]
async fn the_first_upload_to_a_share_that_protects_writes_succeeds() {
    let (server, fs, _dir) = open(Options { auth: Auth::BasicForWrites("pw"), ..Options::default() }).await;

    write_all(fs.as_ref(), "/first.txt", 0, b"first").await.unwrap();

    assert_eq!(std::fs::read(server.root.path().join("first.txt")).unwrap(), b"first");
}
