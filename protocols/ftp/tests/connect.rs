mod support;

use porthmos_ftp::Ftp;
use porthmos_vfs::{Answer, ErrorKind, Protocol, Question};

#[tokio::test]
async fn logs_in_with_the_saved_password_and_lists_the_home_directory() {
    let server = support::start(Some("pw"), None).await;
    std::fs::write(server.root.path().join("hello.txt"), b"hi").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![]);

    let fs = Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, support::USER, Some("pw"), "plain"), &mut prompter)
        .await
        .unwrap();

    let home = fs.home().await.unwrap();
    let names: Vec<String> = fs.list(&home).await.unwrap().into_iter().map(|entry| entry.name).collect();
    assert_eq!(names, ["hello.txt"]);
    assert!(prompter.asked.is_empty());
}

#[tokio::test]
async fn an_empty_username_logs_in_anonymously() {
    let server = support::start(None, None).await;
    let dir = tempfile::tempdir().unwrap();

    let result = Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, "", None, "plain"), &mut support::answers(vec![]))
        .await;

    assert!(result.is_ok(), "{:?}", result.err());
}

#[tokio::test]
async fn a_missing_password_is_asked_once_and_the_answer_logs_in() {
    let server = support::start(Some("pw"), None).await;
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![Some(Answer::Password("pw".into()))]);

    let result = Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, support::USER, None, "plain"), &mut prompter)
        .await;

    assert!(result.is_ok(), "{:?}", result.err());
    assert!(matches!(prompter.asked.as_slice(), [Question::Password { .. }]));
}

#[tokio::test]
async fn cancelling_the_password_prompt_cancels_the_connection() {
    let server = support::start(Some("pw"), None).await;
    let dir = tempfile::tempdir().unwrap();

    let error = Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(server.port, support::USER, None, "plain"), &mut support::answers(vec![None]))
        .await
        .err()
        .unwrap();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Cancelled, "Connection cancelled"));
}

#[tokio::test]
async fn a_second_rejected_password_is_authentication_failed() {
    let server = support::start(Some("pw"), None).await;
    let dir = tempfile::tempdir().unwrap();

    let error = Ftp::new(support::store_path(dir.path()))
        .connect(
            &support::target(server.port, support::USER, None, "plain"),
            &mut support::answers(vec![Some(Answer::Password("nope".into()))]),
        )
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::AuthRejected);
}

#[tokio::test]
async fn an_unreachable_server_is_a_connect_error() {
    let dir = tempfile::tempdir().unwrap();

    let error = Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(1, support::USER, Some("pw"), "plain"), &mut support::answers(vec![]))
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::Connect);
}
