use porthmos_scp::Scp;
use porthmos_ssh::testing::{self, Options, SshServer};
use porthmos_vfs::{Answer, ErrorKind, Protocol, Question};

#[tokio::test]
async fn connects_asking_once_for_the_host_key_and_starts_at_home() {
    let server = SshServer::start(Options::default()).await;
    let mut prompter = testing::answers(vec![Some(Answer::Confirmed)]);

    let fs = Scp::default()
        .with_connect_options(server.connect_options())
        .connect(&server.target(), &mut prompter)
        .await
        .unwrap();

    assert!(matches!(prompter.asked.as_slice(), [Question::TrustHostKey { .. }]));
    assert_eq!(fs.home().await.unwrap(), server.root.path());
}

#[tokio::test]
async fn a_server_refusing_commands_gets_a_clear_message() {
    let server = SshServer::start(Options { refuse_exec: true, ..Options::default() }).await;

    let error = Scp::default()
        .with_connect_options(server.connect_options())
        .connect(&server.target(), &mut testing::answers(vec![Some(Answer::Confirmed)]))
        .await
        .err()
        .unwrap();

    assert_eq!(
        (error.kind(), error.to_string().as_str()),
        (ErrorKind::SessionStart, "the server does not allow running commands (SCP needs a shell)")
    );
}

#[tokio::test]
async fn an_account_without_a_shell_is_refused() {
    let server = SshServer::start(Options { replace: &[("printf 'porthmos", "true")], ..Options::default() }).await;

    let error = Scp::default()
        .with_connect_options(server.connect_options())
        .connect(&server.target(), &mut testing::answers(vec![Some(Answer::Confirmed)]))
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::SessionStart);
}
