mod support;

use std::path::Path;

use porthmos_vfs::{Answer, ErrorKind, Protocol, Question};
use porthmos_webdav::WebDav;
use support::{Auth, Options, Quirk, USER};

fn webdav(dir: &tempfile::TempDir) -> WebDav {
    WebDav::new(dir.path().join("known_certificates.toml"))
}

async fn connect_error(target: porthmos_vfs::Target, answers: Vec<Option<Answer>>) -> (ErrorKind, String) {
    let dir = tempfile::tempdir().unwrap();
    let error = webdav(&dir).connect(&target, &mut support::answers(answers)).await.err().unwrap();
    (error.kind(), error.to_string())
}

#[tokio::test]
async fn an_open_share_connects_anonymously_and_lists() {
    let server = support::start(Options::default()).await;
    std::fs::write(server.root.path().join("hello.txt"), b"hi").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![]);

    let fs = webdav(&dir).connect(&support::target(server.port, "", None), &mut prompter).await.unwrap();

    assert!(prompter.asked.is_empty());
    assert_eq!(fs.home().await.unwrap(), Path::new("/"));
    let names: Vec<String> = fs.read_dir(Path::new("/")).await.unwrap().into_iter().map(|item| item.name).collect();
    assert_eq!(names, vec!["hello.txt".to_string()]);
}

#[tokio::test]
async fn basic_and_digest_accept_the_saved_password_silently() {
    for auth in [Auth::Basic("pw"), Auth::Digest("pw")] {
        let server = support::start(Options { auth, ..Options::default() }).await;
        let dir = tempfile::tempdir().unwrap();
        let mut prompter = support::answers(vec![]);

        let fs = webdav(&dir).connect(&support::target(server.port, USER, Some("pw")), &mut prompter).await.unwrap();

        assert!(prompter.asked.is_empty(), "{auth:?}");
        fs.read_dir(Path::new("/")).await.unwrap();
        fs.read_dir(Path::new("/")).await.unwrap();
    }
}

#[tokio::test]
async fn a_wrong_saved_password_asks_once_and_accepts_the_answer() {
    for auth in [Auth::Basic("pw"), Auth::Digest("pw")] {
        let server = support::start(Options { auth, ..Options::default() }).await;
        let dir = tempfile::tempdir().unwrap();
        let mut prompter = support::answers(vec![Some(Answer::Password("pw".to_string()))]);

        webdav(&dir).connect(&support::target(server.port, USER, Some("wrong")), &mut prompter).await.unwrap();

        assert_eq!(
            prompter.asked,
            vec![Question::Password { username: USER.to_string(), name: "srv".to_string() }],
            "{auth:?}"
        );
    }
}

#[tokio::test]
async fn cancelling_the_password_prompt_cancels_the_connection() {
    let server = support::start(Options { auth: Auth::Basic("pw"), ..Options::default() }).await;

    assert_eq!(
        connect_error(support::target(server.port, USER, None), vec![None]).await,
        (ErrorKind::Cancelled, "Connection cancelled".to_string())
    );
}

#[tokio::test]
async fn a_second_wrong_password_is_rejected() {
    let server = support::start(Options { auth: Auth::Digest("pw"), ..Options::default() }).await;

    assert_eq!(
        connect_error(support::target(server.port, USER, None), vec![Some(Answer::Password("nope".to_string()))]).await,
        (ErrorKind::AuthRejected, "Authentication failed for srv".to_string())
    );
}

#[tokio::test]
async fn an_empty_username_on_a_protected_share_is_rejected() {
    let server = support::start(Options { auth: Auth::Basic("pw"), ..Options::default() }).await;

    assert_eq!(
        connect_error(support::target(server.port, "", None), vec![]).await,
        (ErrorKind::AuthRejected, "the server requires a username".to_string())
    );
}

#[tokio::test]
async fn a_server_that_is_not_webdav_is_named() {
    let server = support::start(Options { quirk: Quirk::NotDav, ..Options::default() }).await;

    assert_eq!(
        connect_error(support::target(server.port, "", None), vec![]).await,
        (ErrorKind::Connect, "127.0.0.1 is not a WebDAV server (PROPFIND returned 200 OK)".to_string())
    );
}

#[tokio::test]
async fn a_missing_root_is_reported() {
    let server = support::start(Options { prefix: "/dav", ..Options::default() }).await;
    let target = support::with_option(support::target(server.port, "", None), "root", "/missing");

    assert_eq!(connect_error(target, vec![]).await, (ErrorKind::Connect, "Root path /missing/ not found".to_string()));
}

#[tokio::test]
async fn a_redirect_is_reported_with_its_location() {
    let server = support::start(Options { quirk: Quirk::RedirectPropfind, ..Options::default() }).await;

    assert_eq!(
        connect_error(support::target(server.port, "", None), vec![]).await,
        (
            ErrorKind::Connect,
            "the server redirected to https://elsewhere.example/dav/ \u{2014} check Security and Root path".to_string()
        )
    );
}

#[tokio::test]
async fn a_closed_port_is_a_connect_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let (kind, _) = connect_error(support::target(port, "", None), vec![]).await;

    assert_eq!(kind, ErrorKind::Connect);
}

#[tokio::test]
async fn a_server_that_never_answers_times_out() {
    let server = support::start(Options { quirk: Quirk::HangPropfind, ..Options::default() }).await;
    let dir = tempfile::tempdir().unwrap();

    let error = webdav(&dir)
        .with_timeout(std::time::Duration::from_millis(200))
        .connect(&support::target(server.port, "", None), &mut support::answers(vec![]))
        .await
        .err()
        .unwrap();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Connect, "the server did not respond in time"));
}

#[tokio::test]
async fn a_share_that_only_protects_writes_uses_the_saved_password() {
    let server = support::start(Options { auth: Auth::BasicForWrites("pw"), ..Options::default() }).await;
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![]);

    let fs = webdav(&dir).connect(&support::target(server.port, USER, Some("pw")), &mut prompter).await.unwrap();
    fs.read_dir(Path::new("/")).await.unwrap();
    fs.create_dir(Path::new("/made")).await.unwrap();

    assert!(prompter.asked.is_empty());
    assert!(server.root.path().join("made").is_dir());
}
