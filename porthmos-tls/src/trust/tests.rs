use std::collections::VecDeque;

use porthmos_vfs::{Answer, ErrorKind, Prompter, Question, async_trait};

use super::*;
use crate::{CertificateDetails, details};

struct Scripted {
    answers: VecDeque<Option<Answer>>,
    asked: Vec<Question>,
}

#[async_trait]
impl Prompter for Scripted {
    async fn ask(&mut self, question: Question) -> Option<Answer> {
        self.asked.push(question);
        self.answers.pop_front().flatten()
    }
}

fn scripted(answers: Vec<Option<Answer>>) -> Scripted {
    Scripted { answers: answers.into(), asked: Vec::new() }
}

fn unknown_certificate() -> CertificateDetails {
    details(rcgen::generate_simple_self_signed(vec!["nas.local".to_string()]).unwrap().cert.der())
}

fn store_in(dir: &tempfile::TempDir) -> KnownCertificates {
    KnownCertificates::new(dir.path().join("known_certificates.toml"))
}

#[tokio::test]
async fn confirming_an_unknown_certificate_remembers_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let certificate = unknown_certificate();
    let mut prompter = scripted(vec![Some(Answer::Confirmed)]);

    ask_to_trust(TrustProblem::Unknown(certificate.clone()), "nas", "nas.local", 443, &store, &mut prompter)
        .await
        .unwrap();

    assert_eq!(store.lookup("nas.local", 443).unwrap(), Some(certificate.fingerprint.clone()));
    assert_eq!(
        prompter.asked,
        vec![Question::TrustCertificate {
            name: "nas".to_string(),
            host: "nas.local".to_string(),
            port: 443,
            fingerprint: certificate.fingerprint,
            subject: certificate.subject,
            expires: certificate.expires,
        }]
    );
}

#[tokio::test]
async fn declining_an_unknown_certificate_cancels_and_remembers_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);

    let error = ask_to_trust(
        TrustProblem::Unknown(unknown_certificate()),
        "nas",
        "nas.local",
        443,
        &store,
        &mut scripted(vec![None]),
    )
    .await
    .unwrap_err();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Cancelled, "Connection cancelled"));
    assert_eq!(store.lookup("nas.local", 443).unwrap(), None);
}

#[tokio::test]
async fn a_changed_certificate_is_refused_without_asking() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let mut prompter = scripted(vec![]);

    let error = ask_to_trust(TrustProblem::Changed, "nas", "nas.local", 443, &store, &mut prompter).await.unwrap_err();

    assert_eq!(error.kind(), ErrorKind::Connect);
    assert_eq!(
        error.to_string(),
        format!("CERTIFICATE CHANGED for nas.local:443 \u{2014} refusing to connect (see {})", store.path().display())
    );
    assert!(prompter.asked.is_empty());
}

#[tokio::test]
async fn an_unreadable_store_is_a_connect_error_with_its_message() {
    let dir = tempfile::tempdir().unwrap();

    let error = ask_to_trust(
        TrustProblem::Store("invalid known_certificates.toml".to_string()),
        "nas",
        "nas.local",
        443,
        &store_in(&dir),
        &mut scripted(vec![]),
    )
    .await
    .unwrap_err();

    assert_eq!((error.kind(), error.to_string().as_str()), (ErrorKind::Connect, "invalid known_certificates.toml"));
}
