use std::{path::Path, sync::Arc, time::Duration};

use porthmos_scp::Scp;
use porthmos_ssh::testing::{self, Options, SshServer};
use porthmos_vfs::{Answer, FileSystem, Protocol};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MIB: usize = 1024 * 1024;

async fn open_with(options: Options, timeout: Duration) -> (SshServer, Arc<dyn FileSystem>) {
    let server = SshServer::start(options).await;
    let fs = Scp::default()
        .with_connect_options(server.connect_options())
        .with_timeout(timeout)
        .connect(&server.target(), &mut testing::answers(vec![Some(Answer::Confirmed)]))
        .await
        .unwrap();
    (server, fs)
}

async fn open(options: Options) -> (SshServer, Arc<dyn FileSystem>) {
    open_with(options, Duration::from_secs(30)).await
}

fn pattern(size: usize) -> Vec<u8> {
    (0..size).map(|index| (index % 251) as u8).collect()
}

async fn read_all(fs: &dyn FileSystem, path: &Path, offset: u64) -> Vec<u8> {
    let mut data = Vec::new();
    fs.open_read(path, offset).await.unwrap().read_to_end(&mut data).await.unwrap();
    data
}

async fn upload(fs: &dyn FileSystem, path: &Path, offset: u64, size: u64, data: &[u8]) -> u64 {
    let mut writer = fs.open_write_sized(path, offset, size).await.unwrap();
    writer.stream.write_all(data).await.unwrap();
    writer.stream.shutdown().await.unwrap();
    writer.offset
}

#[tokio::test]
async fn whole_files_travel_through_scp_both_ways() {
    let (server, fs) = open(Options::default()).await;
    let data = pattern(5 * MIB);
    std::fs::write(server.root.path().join("down.bin"), &data).unwrap();

    assert_eq!(read_all(fs.as_ref(), &server.root.path().join("down.bin"), 0).await, data);
    assert_eq!(upload(fs.as_ref(), &server.root.path().join("up.bin"), 0, data.len() as u64, &data).await, 0);
    assert_eq!(std::fs::read(server.root.path().join("up.bin")).unwrap(), data);
}

#[tokio::test]
async fn transfers_resume_from_an_offset() {
    let (server, fs) = open(Options::default()).await;
    std::fs::write(server.root.path().join("f"), b"hello world").unwrap();
    std::fs::write(server.root.path().join("g.part"), b"hello ").unwrap();

    assert_eq!(read_all(fs.as_ref(), &server.root.path().join("f"), 6).await, b"world");
    assert_eq!(upload(fs.as_ref(), &server.root.path().join("g.part"), 6, 11, b"world").await, 6);
    assert_eq!(std::fs::read(server.root.path().join("g.part")).unwrap(), b"hello world");
}

#[tokio::test]
async fn without_scp_transfers_use_cat() {
    let (server, fs) = open(Options { hide_scp: true, ..Options::default() }).await;
    let data = pattern(2 * MIB);
    std::fs::write(server.root.path().join("down.bin"), &data).unwrap();

    assert_eq!(read_all(fs.as_ref(), &server.root.path().join("down.bin"), 0).await, data);
    upload(fs.as_ref(), &server.root.path().join("up.bin"), 0, data.len() as u64, &data).await;
    assert_eq!(std::fs::read(server.root.path().join("up.bin")).unwrap(), data);
}

#[tokio::test]
async fn an_upload_of_the_wrong_size_fails() {
    let (server, fs) = open(Options::default()).await;

    let mut short = fs.open_write_sized(&server.root.path().join("short"), 0, 10).await.unwrap();
    short.stream.write_all(b"abc").await.unwrap();
    assert_eq!(short.stream.shutdown().await.unwrap_err().to_string(), "the source changed size during the upload");

    let mut long = fs.open_write_sized(&server.root.path().join("long"), 0, 2).await.unwrap();
    assert_eq!(
        long.stream.write_all(b"abc").await.unwrap_err().to_string(),
        "the source changed size during the upload"
    );
}

#[tokio::test]
async fn a_cancelled_upload_resumes_by_appending() {
    let (server, fs) = open(Options::default()).await;
    let data = pattern(MIB);
    let part = server.root.path().join("x.part");

    let mut writer = fs.open_write_sized(&part, 0, data.len() as u64).await.unwrap();
    writer.stream.write_all(&data[..300_000]).await.unwrap();
    let _ = writer.stream.shutdown().await;
    let kept = fs.stat(&part).await.unwrap().size;
    assert!(kept > 0 && kept <= 300_000, "{kept}");

    upload(fs.as_ref(), &part, kept, data.len() as u64, &data[kept as usize..]).await;

    assert_eq!(std::fs::read(&part).unwrap(), data);
}

#[tokio::test]
async fn parallel_transfers_stay_within_the_channel_cap() {
    let (server, fs) = open(Options::default()).await;
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..12u8 {
        let (fs, path) = (fs.clone(), server.root.path().join(format!("p{index}")));
        tasks.spawn(async move {
            let data = vec![index; 200_000];
            upload(fs.as_ref(), &path, 0, data.len() as u64, &data).await;
            assert_eq!(read_all(fs.as_ref(), &path, 0).await, data);
        });
    }
    for _ in 0..6 {
        fs.read_dir(server.root.path()).await.unwrap();
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    assert!(server.peak_commands() <= 8, "{}", server.peak_commands());
}

#[tokio::test]
async fn a_stalled_read_times_out() {
    let (server, fs) = open_with(Options::default(), Duration::from_millis(200)).await;
    let fifo = server.root.path().join("fifo");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());

    let mut reader = fs.open_read(&fifo, 1).await.unwrap();
    let mut buffer = [0u8; 16];
    let error = reader.read(&mut buffer).await.unwrap_err();

    assert_eq!(error.to_string(), "the server did not respond in time");
}

#[tokio::test]
async fn a_fresh_upload_over_a_longer_part_never_keeps_old_bytes() {
    let (server, fs) = open(Options::default()).await;
    let part = server.root.path().join("x.part");
    std::fs::write(&part, vec![b'A'; 500_000]).unwrap();

    let mut writer = fs.open_write_sized(&part, 0, 600_000).await.unwrap();
    writer.stream.write_all(&vec![b'B'; 200_000]).await.unwrap();
    let _ = writer.stream.shutdown().await;

    let kept = std::fs::read(&part).unwrap();
    assert!(kept.len() <= 200_000, "{}", kept.len());
    assert!(kept.iter().all(|byte| *byte == b'B'));
}

#[tokio::test]
async fn cancelled_downloads_release_their_sessions() {
    let (server, fs) = open(Options { max_sessions: Some(10), ..Options::default() }).await;
    let path = server.root.path().join("big.bin");
    std::fs::write(&path, pattern(5 * MIB)).unwrap();

    for _ in 0..12 {
        let mut reader = fs.open_read(&path, 0).await.unwrap();
        let mut first = [0u8; 16];
        reader.read_exact(&mut first).await.unwrap();
        drop(reader);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(fs.read_dir(server.root.path()).await.unwrap().len(), 1);
    assert_eq!(read_all(fs.as_ref(), &path, 0).await.len(), 5 * MIB);
}

#[tokio::test]
async fn a_cat_upload_the_server_refuses_fails_quickly_with_the_reason() {
    let (server, fs) = open_with(Options { hide_scp: true, ..Options::default() }, Duration::from_secs(1)).await;
    let locked = server.root.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o555)).unwrap();
    let data = pattern(3 * MIB);

    let attempt = async {
        let mut writer = fs.open_write_sized(&locked.join("f"), 0, data.len() as u64).await.unwrap();
        let written = writer.stream.write_all(&data).await;
        let closed = writer.stream.shutdown().await;
        written.and(closed).unwrap_err()
    };
    let error = tokio::time::timeout(Duration::from_secs(5), attempt).await.expect("the failure is reported quickly");

    assert!(error.to_string().contains("permission denied"), "{error}");
}

const SCP_WITH_TIME_LINE: &str = "head -c1 >/dev/null; printf 'T1700000000 0 1700000000 0\\n'; head -c1 >/dev/null; printf 'C0644 3 f\\n'; head -c1 >/dev/null; printf 'abc'; printf '\\000'; head -c1 >/dev/null";

#[tokio::test]
async fn a_download_skips_the_time_line_scp_may_send() {
    let (server, fs) = open(Options { replace: &[("scp -f", SCP_WITH_TIME_LINE)], ..Options::default() }).await;

    assert_eq!(read_all(fs.as_ref(), &server.root.path().join("f"), 0).await, b"abc");
}

#[tokio::test]
async fn listing_is_not_starved_by_waiting_transfers() {
    let (server, fs) = open(Options::default()).await;
    let fifo = server.root.path().join("fifo");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let mut stuck = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let (fs, fifo) = (fs.clone(), fifo.clone());
        stuck.spawn(async move {
            let _reader = fs.open_read(&fifo, 1).await;
            std::future::pending::<()>().await;
        });
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    let listed = tokio::time::timeout(Duration::from_secs(5), fs.read_dir(server.root.path())).await;

    assert!(listed.expect("listing is not blocked").is_ok());
    stuck.abort_all();
}

#[tokio::test]
async fn a_refused_scp_upload_keeps_its_reason() {
    let (server, fs) = open(Options::default()).await;
    let locked = server.root.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o555)).unwrap();

    let denied = fs.open_write_sized(&locked.join("f"), 0, 10).await.err().unwrap();
    let missing = fs.open_write_sized(&server.root.path().join("no/such/f"), 0, 10).await.err().unwrap();

    assert!(denied.to_string().contains("permission denied"), "{denied}");
    assert_eq!(missing.kind(), porthmos_vfs::ErrorKind::NotFound, "{missing}");
}

#[tokio::test]
async fn many_finished_transfers_free_their_sessions() {
    let (server, fs) = open(Options { max_sessions: Some(10), ..Options::default() }).await;

    for index in 0..15u8 {
        let path = server.root.path().join(format!("f{index}"));
        upload(fs.as_ref(), &path, 0, 3, b"abc").await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(fs.read_dir(server.root.path()).await.unwrap().len(), 15);
}
