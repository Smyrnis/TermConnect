mod support;

use std::{path::Path, sync::Arc, time::Duration};

use porthmos_s3::S3;
use porthmos_vfs::{FileSystem, Protocol};
use support::{BUCKET, Options, Quirk, SECRET, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MIB: usize = 1024 * 1024;

async fn open(options: Options) -> (Server, Arc<dyn FileSystem>, tempfile::TempDir) {
    let server = support::start(options).await;
    let dir = tempfile::tempdir().unwrap();
    let fs = S3::new(dir.path().join("k.toml"))
        .connect(&support::target(server.port, Some(BUCKET), Some(SECRET)), &mut support::answers(vec![]))
        .await
        .unwrap();
    (server, fs, dir)
}

fn pattern(size: usize) -> Vec<u8> {
    (0..size).map(|index| (index % 251) as u8).collect()
}

async fn read_all(fs: &dyn FileSystem, path: &str, offset: u64) -> Vec<u8> {
    let mut data = Vec::new();
    fs.open_read(Path::new(path), offset).await.unwrap().read_to_end(&mut data).await.unwrap();
    data
}

async fn write(fs: &dyn FileSystem, path: &str, offset: u64, data: &[u8]) -> u64 {
    let mut writer = fs.open_write(Path::new(path), offset).await.unwrap();
    writer.stream.write_all(data).await.unwrap();
    writer.stream.shutdown().await.unwrap();
    writer.offset
}

#[tokio::test]
async fn reads_whole_objects_and_from_an_offset() {
    let (server, fs, _dir) = open(Options::default()).await;
    std::fs::write(server.bucket_dir().join("f"), b"hello world").unwrap();

    assert_eq!(read_all(fs.as_ref(), "/f", 0).await, b"hello world");
    assert_eq!(read_all(fs.as_ref(), "/f", 6).await, b"world");
}

#[tokio::test]
async fn an_upload_spanning_several_parts_completes_at_shutdown() {
    let (server, fs, _dir) = open(Options::default()).await;
    let data = pattern(33 * MIB);

    assert_eq!(write(fs.as_ref(), "/big.bin", 0, &data).await, 0);

    assert_eq!(std::fs::read(server.bucket_dir().join("big.bin")).unwrap(), data);
    assert!(server.registry.pending().is_empty());
}

#[tokio::test]
async fn an_empty_upload_completes() {
    let (server, fs, _dir) = open(Options::default()).await;

    write(fs.as_ref(), "/empty.txt", 0, b"").await;

    assert_eq!(std::fs::read(server.bucket_dir().join("empty.txt")).unwrap(), b"");
}

#[tokio::test]
async fn the_engine_sequence_resumes_and_completes_one_upload() {
    let (server, fs, _dir) = open(Options::default()).await;
    let data = pattern(41 * MIB);

    assert_eq!(write(fs.as_ref(), "/big.bin.part", 0, &data[..20 * MIB]).await, 0);

    assert!(!server.bucket_dir().join("big.bin").exists());
    let listed = fs.read_dir(Path::new("/")).await.unwrap();
    let part = listed.iter().find(|item| item.name == "big.bin.part").unwrap();
    assert_eq!(part.metadata.size, (16 * MIB) as u64);
    let size = fs.stat(Path::new("/big.bin.part")).await.unwrap().size;
    assert_eq!(size, (16 * MIB) as u64);
    let first = server.registry.pending();
    assert_eq!(first.len(), 1);

    assert_eq!(write(fs.as_ref(), "/big.bin.part", size, &data[size as usize..]).await, size);
    assert_eq!(server.registry.pending(), first);

    fs.rename(Path::new("/big.bin.part"), Path::new("/big.bin")).await.unwrap();

    assert_eq!(std::fs::read(server.bucket_dir().join("big.bin")).unwrap(), data);
    assert!(server.registry.pending().is_empty());
    let names: Vec<String> = fs.read_dir(Path::new("/")).await.unwrap().into_iter().map(|item| item.name).collect();
    assert_eq!(names, vec!["big.bin".to_string()]);
}

#[tokio::test]
async fn a_mismatched_offset_restarts_the_upload() {
    let (server, fs, _dir) = open(Options::default()).await;
    write(fs.as_ref(), "/x.part", 0, &pattern(17 * MIB)).await;
    let first = server.registry.pending();

    let mut writer = fs.open_write(Path::new("/x.part"), 5).await.unwrap();

    assert_eq!(writer.offset, 0);
    writer.stream.write_all(b"fresh").await.unwrap();
    writer.stream.shutdown().await.unwrap();
    let now = server.registry.pending();
    assert_eq!(now.len(), 1);
    assert_ne!(now[0].id, first[0].id);
    fs.rename(Path::new("/x.part"), Path::new("/x")).await.unwrap();
    assert_eq!(std::fs::read(server.bucket_dir().join("x")).unwrap(), b"fresh");
}

#[tokio::test]
async fn removing_a_part_aborts_its_upload() {
    let (server, fs, _dir) = open(Options::default()).await;
    write(fs.as_ref(), "/x.part", 0, b"some bytes").await;

    fs.remove_file(Path::new("/x.part")).await.unwrap();

    assert!(server.registry.pending().is_empty());
    assert!(fs.read_dir(Path::new("/")).await.unwrap().is_empty());
}

#[tokio::test]
async fn parallel_uploads_run_while_listing() {
    let (server, fs, _dir) = open(Options::default()).await;
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..4u8 {
        let fs = fs.clone();
        tasks.spawn(async move {
            write(fs.as_ref(), &format!("/p{index}"), 0, &vec![index; MIB]).await;
        });
    }
    for _ in 0..4 {
        fs.read_dir(Path::new("/")).await.unwrap();
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    for index in 0..4u8 {
        assert_eq!(std::fs::read(server.bucket_dir().join(format!("p{index}"))).unwrap(), vec![index; MIB]);
    }
}

#[tokio::test]
async fn a_read_the_server_never_answers_times_out() {
    let server = support::start(Options { quirk: Quirk::HangGet, ..Options::default() }).await;
    std::fs::write(server.bucket_dir().join("f"), b"f").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let fs = S3::new(dir.path().join("k.toml"))
        .with_timeout(Duration::from_millis(100))
        .connect(&support::target(server.port, Some(BUCKET), Some(SECRET)), &mut support::answers(vec![]))
        .await
        .unwrap();

    let error = fs.open_read(Path::new("/f"), 0).await.err().unwrap();

    assert_eq!(error.to_string(), "the server did not respond in time");
}

#[tokio::test]
async fn uploads_work_for_a_key_that_cannot_list_unfinished_uploads() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::DenyUploadListing, ..Options::default() }).await;
    let data = pattern(MIB);

    write(fs.as_ref(), "/x.bin.part", 0, &data).await;
    fs.rename(Path::new("/x.bin.part"), Path::new("/x.bin")).await.unwrap();

    assert_eq!(std::fs::read(server.bucket_dir().join("x.bin")).unwrap(), data);
    assert!(server.registry.pending().is_empty());
}

#[tokio::test]
async fn a_cancelled_upload_is_cleaned_up_without_upload_listing() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::DenyUploadListing, ..Options::default() }).await;

    write(fs.as_ref(), "/x.bin.part", 0, b"partial").await;
    fs.remove_file(Path::new("/x.bin.part")).await.unwrap();

    assert!(server.registry.pending().is_empty());
}

#[tokio::test]
async fn uploads_work_and_folders_list_for_a_key_that_cannot_list_parts() {
    let (server, fs, _dir) = open(Options { quirk: Quirk::DenyPartListing, ..Options::default() }).await;
    let data = pattern(MIB);

    write(fs.as_ref(), "/x.bin.part", 0, &data).await;
    let names: Vec<String> = fs.read_dir(Path::new("/")).await.unwrap().into_iter().map(|item| item.name).collect();
    assert_eq!(names, vec!["x.bin.part".to_string()]);
    fs.rename(Path::new("/x.bin.part"), Path::new("/x.bin")).await.unwrap();

    assert_eq!(std::fs::read(server.bucket_dir().join("x.bin")).unwrap(), data);
}
