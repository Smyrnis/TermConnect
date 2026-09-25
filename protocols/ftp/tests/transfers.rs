mod support;

use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};

use porthmos_ftp::Ftp;
use porthmos_vfs::{FileSystem, Protocol};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn connect_to(server: &support::Server) -> Arc<dyn FileSystem> {
    let dir = tempfile::tempdir().unwrap();
    Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, support::USER, Some("pw"), "plain"), &mut support::answers(vec![]))
        .await
        .unwrap()
}

async fn read_all(fs: &Arc<dyn FileSystem>, path: &str, offset: u64) -> Vec<u8> {
    let mut reader = fs.open_read(Path::new(path), offset).await.unwrap();
    let mut data = Vec::new();
    reader.read_to_end(&mut data).await.unwrap();
    data
}

#[tokio::test]
async fn reads_a_whole_file() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("a.txt"), b"hello world").unwrap();
    let fs = connect_to(&server).await;

    assert_eq!(read_all(&fs, "/a.txt", 0).await, b"hello world");
}

#[tokio::test]
async fn reads_from_an_offset() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("a.txt"), b"hello world").unwrap();
    let fs = connect_to(&server).await;

    assert_eq!(read_all(&fs, "/a.txt", 6).await, b"world");
}

#[tokio::test]
async fn writes_a_new_file() {
    let server = support::start(Some("pw"), None).await;
    let fs = connect_to(&server).await;

    let mut writer = fs.open_write(Path::new("/new.bin"), 0).await.unwrap();
    writer.stream.write_all(b"payload").await.unwrap();
    writer.stream.shutdown().await.unwrap();

    assert_eq!(writer.offset, 0);
    assert_eq!(std::fs::read(server.root.path().join("new.bin")).unwrap(), b"payload");
}

#[tokio::test]
async fn writing_at_an_offset_resumes_the_file() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("part.bin"), b"hello ").unwrap();
    let fs = connect_to(&server).await;

    let mut writer = fs.open_write(Path::new("/part.bin"), 6).await.unwrap();
    writer.stream.write_all(b"world").await.unwrap();
    writer.stream.shutdown().await.unwrap();

    assert_eq!(writer.offset, 6);
    assert_eq!(std::fs::read(server.root.path().join("part.bin")).unwrap(), b"hello world");
}

#[tokio::test]
async fn a_write_the_server_rejects_is_an_error() {
    let server = support::start(Some("pw"), None).await;
    let locked = server.root.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let fs = connect_to(&server).await;

    let outcome = match fs.open_write(Path::new("/locked/f.bin"), 0).await {
        Err(_) => Err(()),
        Ok(mut writer) => {
            let _ = writer.stream.write_all(b"data").await;
            writer.stream.shutdown().await.map_err(|_| ())
        }
    };

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(outcome.is_err());
}

#[tokio::test]
async fn a_cancelled_download_does_not_poison_the_next_transfer() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("big.bin"), vec![7u8; 4 * 1024 * 1024]).unwrap();
    std::fs::write(server.root.path().join("small.txt"), b"small").unwrap();
    let fs = connect_to(&server).await;

    let mut reader = fs.open_read(Path::new("/big.bin"), 0).await.unwrap();
    let mut chunk = vec![0u8; 64 * 1024];
    reader.read_exact(&mut chunk).await.unwrap();
    drop(reader);

    assert_eq!(read_all(&fs, "/small.txt", 0).await, b"small");
    assert_eq!(read_all(&fs, "/small.txt", 0).await, b"small");
}

#[tokio::test]
async fn several_transfers_run_while_listing() {
    let server = support::start(Some("pw"), None).await;
    for index in 0..4 {
        std::fs::write(server.root.path().join(format!("f{index}.bin")), vec![index as u8; 256 * 1024]).unwrap();
    }
    let fs = connect_to(&server).await;

    let (a, b, c, d, listing) = tokio::join!(
        read_all(&fs, "/f0.bin", 0),
        read_all(&fs, "/f1.bin", 0),
        read_all(&fs, "/f2.bin", 0),
        read_all(&fs, "/f3.bin", 0),
        fs.list(Path::new("/")),
    );

    assert_eq!([a.len(), b.len(), c.len(), d.len()], [256 * 1024; 4]);
    assert!(d.iter().all(|byte| *byte == 3));
    assert_eq!(listing.unwrap().len(), 4);
}
