mod support;

use std::path::Path;

use porthmos_vfs::{Answer, ErrorKind, Protocol, Question, Target};
use porthmos_webdav::WebDav;
use support::Options;

fn https_target(port: u16) -> Target {
    support::with_option(support::target(port, "", None), "security", "https")
}

#[tokio::test]
async fn a_self_signed_certificate_is_asked_once_then_trusted_silently() {
    let tls = support::self_signed_tls();
    let server = support::start_with_tls(Options::default(), Some(&tls)).await;
    std::fs::write(server.root.path().join("secret.txt"), b"over tls").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("known_certificates.toml");
    let mut first = support::answers(vec![Some(Answer::Confirmed)]);

    WebDav::new(store.clone()).connect(&https_target(server.port), &mut first).await.unwrap();

    assert!(matches!(first.asked.as_slice(), [Question::TrustCertificate { port, .. }] if *port == server.port));
    let mut second = support::answers(vec![]);
    let fs = WebDav::new(store).connect(&https_target(server.port), &mut second).await.unwrap();
    assert!(second.asked.is_empty());
    assert_eq!(fs.read_dir(Path::new("/")).await.unwrap()[0].name, "secret.txt");
}

#[tokio::test]
async fn declining_the_certificate_cancels_and_remembers_nothing() {
    let tls = support::self_signed_tls();
    let server = support::start_with_tls(Options::default(), Some(&tls)).await;
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("known_certificates.toml");

    let error = WebDav::new(store.clone())
        .connect(&https_target(server.port), &mut support::answers(vec![None]))
        .await
        .err()
        .unwrap();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Cancelled, "Connection cancelled"));
    assert!(!store.exists());
}

#[tokio::test]
async fn a_changed_certificate_is_refused_without_asking() {
    let old = support::self_signed_tls();
    let new = support::self_signed_tls();
    let old_server = support::start_with_tls(Options::default(), Some(&old)).await;
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("known_certificates.toml");
    WebDav::new(store.clone())
        .connect(&https_target(old_server.port), &mut support::answers(vec![Some(Answer::Confirmed)]))
        .await
        .unwrap();
    let new_server = support::start_with_tls(Options::default(), Some(&new)).await;
    let pinned = std::fs::read_to_string(&store).unwrap();
    std::fs::write(&store, pinned.replace(&old_server.port.to_string(), &new_server.port.to_string())).unwrap();
    let mut prompter = support::answers(vec![]);

    let error = WebDav::new(store).connect(&https_target(new_server.port), &mut prompter).await.err().unwrap();

    assert_eq!(error.kind(), ErrorKind::Connect);
    assert!(
        error.to_string().starts_with(&format!("CERTIFICATE CHANGED for 127.0.0.1:{}", new_server.port)),
        "{error}"
    );
    assert!(prompter.asked.is_empty());
}
