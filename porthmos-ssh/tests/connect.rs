use porthmos_ssh::{
    connect, exec, open_exec,
    testing::{self, Options, SshServer},
};
use porthmos_vfs::{Answer, ErrorKind, Question, Target};
use tokio::io::AsyncReadExt;

fn target(port: u16, password: Option<&str>) -> Target {
    Target {
        name: "box".into(),
        host: "127.0.0.1".into(),
        port,
        username: "u".into(),
        password: password.map(str::to_string),
        options: Default::default(),
    }
}

#[tokio::test]
async fn an_unknown_host_key_is_asked_once_then_trusted() {
    let server = SshServer::start(Options::default()).await;
    let mut first = testing::answers(vec![Some(Answer::Confirmed)]);

    connect(&target(server.port, Some(testing::PASSWORD)), &mut first, &server.connect_options()).await.unwrap();

    assert!(matches!(first.asked.as_slice(), [Question::TrustHostKey { port, .. }] if *port == server.port));
    let mut second = testing::answers(vec![]);
    connect(&target(server.port, Some(testing::PASSWORD)), &mut second, &server.connect_options()).await.unwrap();
    assert!(second.asked.is_empty());
}

#[tokio::test]
async fn a_wrong_password_asks_once_and_accepts_the_answer() {
    let server = SshServer::start(Options::default()).await;
    let mut prompter =
        testing::answers(vec![Some(Answer::Confirmed), Some(Answer::Password(testing::PASSWORD.to_string()))]);

    connect(&target(server.port, Some("nope")), &mut prompter, &server.connect_options()).await.unwrap();

    assert!(matches!(prompter.asked.as_slice(), [Question::TrustHostKey { .. }, Question::Password { .. }]));
}

#[tokio::test]
async fn cancelling_the_password_prompt_cancels() {
    let server = SshServer::start(Options::default()).await;

    let error = connect(
        &target(server.port, None),
        &mut testing::answers(vec![Some(Answer::Confirmed), None]),
        &server.connect_options(),
    )
    .await
    .err()
    .unwrap();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Cancelled, "Connection cancelled"));
}

#[tokio::test]
async fn a_second_wrong_password_is_rejected() {
    let server = SshServer::start(Options::default()).await;
    let answers = vec![Some(Answer::Confirmed), Some(Answer::Password("still wrong".to_string()))];

    let error = connect(&target(server.port, Some("nope")), &mut testing::answers(answers), &server.connect_options())
        .await
        .err()
        .unwrap();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::AuthRejected, "Authentication failed for box"));
}

#[tokio::test]
async fn exec_returns_output_errors_and_status() {
    let server = SshServer::start(Options::default()).await;
    let session = server.session().await;

    let output = exec(&session, "printf hi; echo oops >&2; exit 3").await.unwrap();

    assert_eq!(
        (output.stdout.as_slice(), output.stderr.as_slice(), output.status),
        (b"hi".as_slice(), b"oops\n".as_slice(), Some(3))
    );
}

#[tokio::test]
async fn commands_run_in_the_server_directory() {
    let server = SshServer::start(Options::default()).await;
    std::fs::write(server.root.path().join("here.txt"), b"x").unwrap();
    let session = server.session().await;

    let output = exec(&session, "ls").await.unwrap();

    assert_eq!(String::from_utf8_lossy(&output.stdout), "here.txt\n");
}

#[tokio::test]
async fn an_exec_channel_streams_both_ways() {
    let server = SshServer::start(Options::default()).await;
    let session = server.session().await;
    let data: Vec<u8> = (0..300_000u32).map(|index| (index % 251) as u8).collect();

    let mut channel = open_exec(&session, "cat").await.unwrap();
    let (input, output) = (&channel.input, &mut channel.output);
    let writer = async {
        input.send(&data).await.unwrap();
        input.close().await.unwrap();
    };
    let mut received = Vec::new();
    let reader = async {
        output.read_to_end(&mut received).await.unwrap();
    };
    tokio::join!(writer, reader);

    assert_eq!(received, data);
    assert_eq!(channel.finish().await.status, Some(0));
}

#[tokio::test]
async fn a_server_refusing_commands_fails_the_exec() {
    let server = SshServer::start(Options { refuse_exec: true, ..Options::default() }).await;
    let session = server.session().await;

    assert!(exec(&session, "true").await.is_err());
}
