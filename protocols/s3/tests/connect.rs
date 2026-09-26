mod support;

use std::path::Path;

use porthmos_s3::S3;
use porthmos_vfs::{Answer, ErrorKind, Protocol, Question, Target};
use support::{ACCESS_KEY, BUCKET, Options, Quirk, SECRET};

fn s3(dir: &tempfile::TempDir) -> S3 {
    S3::new(dir.path().join("known_certificates.toml"))
}

async fn connect_error(target: Target, answers: Vec<Option<Answer>>) -> (ErrorKind, String) {
    let dir = tempfile::tempdir().unwrap();
    let error = s3(&dir).connect(&target, &mut support::answers(answers)).await.err().unwrap();
    (error.kind(), error.to_string())
}

#[tokio::test]
async fn without_a_bucket_the_root_lists_buckets() {
    let server = support::start(Options { buckets: &["bkt", "other"], ..Options::default() }).await;
    let dir = tempfile::tempdir().unwrap();

    let fs = s3(&dir)
        .connect(&support::target(server.port, None, Some(SECRET)), &mut support::answers(vec![]))
        .await
        .unwrap();

    let mut items = fs.read_dir(Path::new("/")).await.unwrap();
    items.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(
        items.iter().map(|item| (item.name.as_str(), item.metadata.is_dir())).collect::<Vec<_>>(),
        vec![("bkt", true), ("other", true)]
    );
    assert_eq!(items[0].path, Path::new("/bkt"));
}

#[tokio::test]
async fn a_configured_bucket_is_the_root() {
    let server = support::start(Options::default()).await;
    std::fs::write(server.bucket_dir().join("hello.txt"), b"hi").unwrap();
    let dir = tempfile::tempdir().unwrap();

    let fs = s3(&dir)
        .connect(&support::target(server.port, Some(BUCKET), Some(SECRET)), &mut support::answers(vec![]))
        .await
        .unwrap();

    assert_eq!(fs.home().await.unwrap(), Path::new("/"));
    let items = fs.read_dir(Path::new("/")).await.unwrap();
    assert_eq!(items.iter().map(|item| item.path.as_path()).collect::<Vec<_>>(), vec![Path::new("/hello.txt")]);
}

#[tokio::test]
async fn a_wrong_saved_secret_asks_once_then_connects() {
    let server = support::start(Options::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![Some(Answer::Password(SECRET.to_string()))]);

    s3(&dir).connect(&support::target(server.port, Some(BUCKET), Some("nope")), &mut prompter).await.unwrap();

    assert_eq!(
        prompter.asked,
        vec![Question::Password { username: ACCESS_KEY.to_string(), name: "store".to_string() }]
    );
}

#[tokio::test]
async fn a_missing_secret_is_asked_for_first() {
    let server = support::start(Options::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![Some(Answer::Password(SECRET.to_string()))]);

    s3(&dir).connect(&support::target(server.port, Some(BUCKET), None), &mut prompter).await.unwrap();

    assert_eq!(prompter.asked.len(), 1);
}

#[tokio::test]
async fn cancelling_the_secret_prompt_cancels() {
    let server = support::start(Options::default()).await;

    assert_eq!(
        connect_error(support::target(server.port, Some(BUCKET), None), vec![None]).await,
        (ErrorKind::Cancelled, "Connection cancelled".to_string())
    );
}

#[tokio::test]
async fn a_second_wrong_secret_is_rejected() {
    let server = support::start(Options::default()).await;

    assert_eq!(
        connect_error(
            support::target(server.port, Some(BUCKET), Some("nope")),
            vec![Some(Answer::Password("still nope".to_string()))]
        )
        .await,
        (ErrorKind::AuthRejected, "Authentication failed for store".to_string())
    );
}

#[tokio::test]
async fn a_missing_bucket_is_named() {
    let server = support::start(Options::default()).await;

    assert_eq!(
        connect_error(support::target(server.port, Some("nope"), Some(SECRET)), vec![]).await,
        (ErrorKind::Connect, "Bucket nope not found".to_string())
    );
}

#[tokio::test]
async fn a_region_redirect_is_followed_once() {
    let server = support::start(Options { quirk: Quirk::RegionRedirect("eu-west-1"), ..Options::default() }).await;
    std::fs::write(server.bucket_dir().join("f"), b"f").unwrap();
    let dir = tempfile::tempdir().unwrap();

    let fs = s3(&dir)
        .connect(&support::target(server.port, Some(BUCKET), Some(SECRET)), &mut support::answers(vec![]))
        .await
        .unwrap();

    assert_eq!(fs.read_dir(Path::new("/")).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_key_that_cannot_list_buckets_gets_a_hint() {
    let server = support::start(Options { quirk: Quirk::DenyListBuckets, ..Options::default() }).await;

    assert_eq!(
        connect_error(support::target(server.port, None, Some(SECRET)), vec![]).await,
        (ErrorKind::Connect, "This key can't list buckets \u{2014} enter a Bucket name".to_string())
    );
}

#[tokio::test]
async fn a_skewed_clock_is_explained() {
    let server = support::start(Options { quirk: Quirk::ClockSkew, ..Options::default() }).await;

    assert_eq!(
        connect_error(support::target(server.port, Some(BUCKET), Some(SECRET)), vec![]).await,
        (ErrorKind::Connect, "the server's clock differs from this computer's by more than 15 minutes".to_string())
    );
}

#[tokio::test]
async fn a_closed_port_is_a_connect_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let (kind, _) = connect_error(support::target(port, Some(BUCKET), Some(SECRET)), vec![]).await;

    assert_eq!(kind, ErrorKind::Connect);
}
