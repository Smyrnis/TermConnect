mod support;

use std::path::Path;

use porthmos_ftp::Ftp;
use porthmos_vfs::{Answer, ErrorKind, Protocol, Question};
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn explicit_ftps_asks_once_then_trusts_silently_and_transfers_over_tls() {
    let tls = support::self_signed_tls("localhost");
    let server = support::start(Some("pw"), Some(&tls)).await;
    std::fs::write(server.root.path().join("secret.txt"), b"over tls").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let target = support::target(server.port, support::USER, Some("pw"), "explicit");
    let mut first = support::answers(vec![Some(Answer::Confirmed)]);

    Ftp::new(support::store_path(dir.path())).connect(&target, &mut first).await.unwrap();

    assert!(matches!(first.asked.as_slice(), [Question::TrustCertificate { port, .. }] if *port == server.port));
    assert!(support::store_path(dir.path()).exists());

    let mut second = support::answers(vec![]);
    let fs = Ftp::new(support::store_path(dir.path())).connect(&target, &mut second).await.unwrap();
    assert!(second.asked.is_empty());

    let mut reader = fs.open_read(Path::new("/secret.txt"), 0).await.unwrap();
    let mut data = Vec::new();
    reader.read_to_end(&mut data).await.unwrap();
    assert_eq!(data, b"over tls");
}

#[tokio::test]
async fn declining_the_certificate_cancels_and_remembers_nothing() {
    let tls = support::self_signed_tls("localhost");
    let server = support::start(Some("pw"), Some(&tls)).await;
    let dir = tempfile::tempdir().unwrap();

    let error = Ftp::new(support::store_path(dir.path()))
        .connect(
            &support::target(server.port, support::USER, Some("pw"), "explicit"),
            &mut support::answers(vec![None]),
        )
        .await
        .err()
        .unwrap();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Cancelled, "Connection cancelled"));
    assert!(!support::store_path(dir.path()).exists());
}

#[tokio::test]
async fn a_changed_certificate_is_refused_without_asking() {
    let old = support::self_signed_tls("localhost");
    let new = support::self_signed_tls("localhost");
    let old_server = support::start(Some("pw"), Some(&old)).await;
    let dir = tempfile::tempdir().unwrap();
    let store = support::store_path(dir.path());
    Ftp::new(store.clone())
        .connect(
            &support::target(old_server.port, support::USER, Some("pw"), "explicit"),
            &mut support::answers(vec![Some(Answer::Confirmed)]),
        )
        .await
        .unwrap();
    let pinned = std::fs::read_to_string(&store).unwrap();
    let new_server = support::start(Some("pw"), Some(&new)).await;
    std::fs::write(&store, pinned.replace(&old_server.port.to_string(), &new_server.port.to_string())).unwrap();
    let mut prompter = support::answers(vec![]);

    let error = Ftp::new(store.clone())
        .connect(&support::target(new_server.port, support::USER, Some("pw"), "explicit"), &mut prompter)
        .await
        .err()
        .unwrap();

    assert_eq!(error.kind(), ErrorKind::Connect);
    assert!(
        error.to_string().starts_with(&format!("CERTIFICATE CHANGED for 127.0.0.1:{}", new_server.port)),
        "{error}"
    );
    assert!(prompter.asked.is_empty());
}

#[tokio::test]
async fn implicit_ftps_asks_to_trust_and_logs_in() {
    let tls = support::self_signed_tls("localhost");
    let port = support::start_implicit(&tls).await;
    let dir = tempfile::tempdir().unwrap();
    let mut prompter = support::answers(vec![Some(Answer::Confirmed)]);

    let fs = Ftp::new(support::store_path(dir.path()))
        .connect(&support::target(port, support::USER, Some("pw"), "implicit"), &mut prompter)
        .await
        .unwrap();

    assert!(matches!(prompter.asked.as_slice(), [Question::TrustCertificate { .. }]));
    assert_eq!(fs.home().await.unwrap(), Path::new("/"));
}
