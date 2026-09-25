mod support;

use std::{path::Path, sync::Arc, time::Duration};

use porthmos_ftp::Ftp;
use porthmos_vfs::{Answer, ErrorKind, FileKind, FileSystem, Protocol};
use support::scripted::{self, Node, Script};
use tokio::io::AsyncWriteExt;

const TEST_TIMEOUT: Duration = Duration::from_millis(400);

async fn connect(port: u16) -> Arc<dyn FileSystem> {
    let dir = tempfile::tempdir().unwrap();
    Ftp::new(support::store_path(dir.path()))
        .with_command_timeout(TEST_TIMEOUT)
        .connect(&support::target(port, support::USER, Some("pw"), "plain"), &mut support::answers(vec![]))
        .await
        .unwrap()
}

fn is_file(node: Option<Node>) -> bool {
    matches!(node, Some(Node::File(_)))
}

#[tokio::test]
async fn a_list_only_server_lists_files_and_directories() {
    let server = scripted::start(Script::default()).await;
    server.file("/a.txt", b"12345");
    server.dir("/photos");
    let fs = connect(server.port).await;

    let mut entries = fs.list(Path::new("/")).await.unwrap();
    entries.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(
        entries.iter().map(|e| (e.name.as_str(), e.is_dir, e.size)).collect::<Vec<_>>(),
        [("a.txt", false, 5), ("photos", true, 0)]
    );
}

#[tokio::test]
async fn a_symlinked_directory_is_browsable_but_read_dir_reports_it_as_a_link() {
    let server = scripted::start(Script::default()).await;
    server.dir("/archive");
    server.link("/shared", "/archive");
    let fs = connect(server.port).await;

    let listed = fs.list(Path::new("/")).await.unwrap();
    let items = fs.read_dir(Path::new("/")).await.unwrap();

    assert!(listed.iter().any(|entry| entry.name == "shared" && entry.is_dir));
    assert!(items.iter().any(|item| item.name == "shared" && item.metadata.kind == FileKind::Symlink));
}

#[tokio::test]
async fn deleting_a_tree_removes_a_symlink_inside_it_but_never_the_links_target() {
    let server = scripted::start(Script::default()).await;
    server.dir("/archive");
    server.file("/archive/keep.txt", b"keep");
    server.dir("/project");
    server.file("/project/a.txt", b"a");
    server.link("/project/shared", "/archive");
    let fs = connect(server.port).await;

    fs.delete(Path::new("/project")).await.unwrap();

    assert!(server.get("/project").is_none());
    assert!(server.get("/project/shared").is_none());
    assert!(is_file(server.get("/archive/keep.txt")));
}

#[tokio::test]
async fn deleting_a_symlink_to_a_directory_removes_only_the_link() {
    let server = scripted::start(Script::default()).await;
    server.dir("/archive");
    server.file("/archive/keep.txt", b"keep");
    server.link("/shared", "/archive");
    let fs = connect(server.port).await;

    fs.delete(Path::new("/shared")).await.unwrap();

    assert!(server.get("/shared").is_none());
    assert!(is_file(server.get("/archive/keep.txt")));
}

#[tokio::test]
async fn a_failed_rename_never_touches_an_existing_backup_file() {
    let server = scripted::start(Script::default()).await;
    server.file("/target.txt", b"target");
    server.file("/target.txt.bak", b"precious");
    let fs = connect(server.port).await;

    let result = fs.rename(Path::new("/missing.txt"), Path::new("/target.txt")).await;

    assert!(result.is_err());
    assert!(matches!(server.get("/target.txt.bak"), Some(Node::File(data)) if data == b"precious"));
    assert!(matches!(server.get("/target.txt"), Some(Node::File(data)) if data == b"target"));
}

#[tokio::test]
async fn a_server_refusing_rest_and_appe_restarts_the_upload_from_zero() {
    let server = scripted::start(Script { refuse_rest: true, refuse_append: true, ..Script::default() }).await;
    server.file("/part.bin", b"stale");
    let fs = connect(server.port).await;

    let mut writer = fs.open_write(Path::new("/part.bin"), 5).await.unwrap();
    writer.stream.write_all(b"whole file").await.unwrap();
    writer.stream.shutdown().await.unwrap();

    assert_eq!(writer.offset, 0);
    assert!(matches!(server.get("/part.bin"), Some(Node::File(data)) if data == b"whole file"));
}

#[tokio::test]
async fn a_login_refused_because_encryption_is_required_says_so_without_asking_for_a_password() {
    let server = scripted::start(Script {
        login_reply: Some("530 Non-anonymous sessions must use encryption."),
        ..Script::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![Some(Answer::Password("pw".into()))]);

    let error = Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, support::USER, Some("pw"), "plain"), &mut prompter)
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::Auth);
    assert!(error.to_string().contains("must use encryption"), "{error}");
    assert!(prompter.asked.is_empty());
}

#[tokio::test]
async fn a_server_that_stops_answering_times_out_instead_of_hanging() {
    let server = scripted::start(Script { silent_after_login: true, ..Script::default() }).await;
    let fs = connect(server.port).await;

    let result = tokio::time::timeout(Duration::from_secs(10), fs.list(Path::new("/"))).await;

    let error = result.expect("the listing must give up by itself").unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
}

#[tokio::test]
async fn an_invalid_certificate_store_fails_the_connection_naming_the_file() {
    let tls = support::self_signed_tls("localhost");
    let server = support::start(Some("pw"), Some(&tls)).await;
    let dir = tempfile::tempdir().unwrap();
    let store = support::store_path(dir.path());
    std::fs::write(&store, "= = not toml").unwrap();
    let mut prompter = support::answers(vec![Some(Answer::Confirmed)]);

    let error = Ftp::new(store.clone())
        .connect(&support::target(server.port, support::USER, Some("pw"), "explicit"), &mut prompter)
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::Connect);
    assert!(error.to_string().contains(&store.display().to_string()), "{error}");
    assert!(prompter.asked.is_empty());
}

#[tokio::test]
async fn an_unparseable_reply_does_not_throw_away_a_healthy_connection() {
    let server = scripted::start(Script { mdtm_reply: Some("213 not-a-date"), ..Script::default() }).await;
    server.file("/a.txt", b"12345");
    let fs = connect(server.port).await;

    for _ in 0..3 {
        assert_eq!(fs.stat(Path::new("/a.txt")).await.unwrap().size, 5);
    }

    assert_eq!(server.connection_count(), 1);
}

#[tokio::test]
async fn a_slow_large_listing_is_allowed_to_finish() {
    let server = scripted::start(Script { listing_delay: Some(Duration::from_millis(250)), ..Script::default() }).await;
    for index in 0..5 {
        server.file(&format!("/f{index}.txt"), b"a");
    }
    let fs = connect(server.port).await;

    let entries = fs.list(Path::new("/")).await.unwrap();

    assert_eq!(entries.len(), 5);
}

#[tokio::test]
async fn a_missing_password_is_asked_for_before_any_login_attempt() {
    let server = scripted::start(Script::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![Some(Answer::Password("typed".into()))]);

    Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, support::USER, None, "plain"), &mut prompter)
        .await
        .unwrap();

    assert_eq!(*server.passwords.lock().unwrap(), ["typed"]);
}

#[tokio::test]
async fn a_server_that_never_answers_the_login_times_out() {
    let server = scripted::start(Script { silent_from_start: true, ..Script::default() }).await;
    let dir = tempfile::tempdir().unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(10),
        Ftp::new(support::store_path(dir.path()))
            .with_command_timeout(TEST_TIMEOUT)
            .connect(&support::target(server.port, support::USER, Some("pw"), "plain"), &mut support::answers(vec![])),
    )
    .await;

    let error = result.expect("connecting must give up by itself").err().unwrap();
    assert!(error.to_string().contains("timed out logging in"), "{error}");
}
