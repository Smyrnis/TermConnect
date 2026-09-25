use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

use super::*;

fn self_signed(name: &str) -> Vec<u8> {
    rcgen::generate_simple_self_signed(vec![name.to_string()]).unwrap().cert.der().to_vec()
}

fn store_in(dir: &tempfile::TempDir) -> KnownCertificates {
    KnownCertificates::new(dir.path().join("known_certificates.toml"))
}

fn verify(verifier: &TrustVerifier, der: &[u8]) -> Result<(), rustls::Error> {
    verifier
        .verify_server_cert(
            &CertificateDer::from(der.to_vec()),
            &[],
            &ServerName::try_from("nas.local").unwrap(),
            &[],
            UnixTime::now(),
        )
        .map(|_| ())
}

#[test]
fn details_give_a_sha256_fingerprint_subject_and_expiry() {
    let details = details(&self_signed("nas.local"));

    assert!(details.fingerprint.starts_with("SHA256:"), "{}", details.fingerprint);
    assert!(!details.fingerprint.ends_with('='));
    assert_eq!(details.fingerprint.len(), "SHA256:".len() + 43);
    assert!(details.subject.contains("CN="), "{}", details.subject);
    assert_eq!(details.expires.len(), 10);
    assert_eq!(&details.expires[4..5], "-");
}

#[test]
fn a_missing_store_file_knows_nothing() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(store_in(&dir).lookup("nas", 21).unwrap(), None);
}

#[test]
fn remembered_certificates_are_found_by_host_and_port() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let details = details(&self_signed("nas.local"));

    store.remember("nas", 21, &details).unwrap();

    assert_eq!(store.lookup("nas", 21).unwrap(), Some(details.fingerprint.clone()));
    assert_eq!(store.lookup("nas", 990).unwrap(), None);
    assert!(!dir.path().join("known_certificates.toml.tmp").exists());
}

#[test]
fn remembering_keeps_the_other_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    store.remember("a", 21, &details(&self_signed("a"))).unwrap();
    store.remember("b", 21, &details(&self_signed("b"))).unwrap();

    assert!(store.lookup("a", 21).unwrap().is_some());
    assert!(store.lookup("b", 21).unwrap().is_some());
}

#[test]
fn an_invalid_store_file_is_an_error_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("known_certificates.toml");
    std::fs::write(&path, "this is = = not toml").unwrap();

    let error = KnownCertificates::new(path.clone()).lookup("nas", 21).unwrap_err();

    assert!(error.to_string().contains(&path.display().to_string()), "{error}");
}

#[test]
fn an_unknown_self_signed_certificate_is_rejected_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let der = self_signed("nas.local");
    let (verifier, problem) = TrustVerifier::new("nas", 21, store_in(&dir));

    assert!(verify(&verifier, &der).is_err());
    assert_eq!(*problem.lock().unwrap(), Some(TrustProblem::Unknown(details(&der))));
}

#[test]
fn a_remembered_certificate_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let der = self_signed("nas.local");
    store_in(&dir).remember("nas", 21, &details(&der)).unwrap();
    let (verifier, problem) = TrustVerifier::new("nas", 21, store_in(&dir));

    assert!(verify(&verifier, &der).is_ok());
    assert_eq!(*problem.lock().unwrap(), None);
}

#[test]
fn a_different_certificate_than_the_remembered_one_is_changed() {
    let dir = tempfile::tempdir().unwrap();
    store_in(&dir).remember("nas", 21, &details(&self_signed("old"))).unwrap();
    let (verifier, problem) = TrustVerifier::new("nas", 21, store_in(&dir));

    assert!(verify(&verifier, &self_signed("new")).is_err());
    assert_eq!(*problem.lock().unwrap(), Some(TrustProblem::Changed));
}

#[test]
fn an_unreadable_store_is_reported_as_a_store_problem() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("known_certificates.toml"), "= =").unwrap();
    let (verifier, problem) = TrustVerifier::new("nas", 21, store_in(&dir));

    assert!(verify(&verifier, &self_signed("nas.local")).is_err());
    assert!(matches!(*problem.lock().unwrap(), Some(TrustProblem::Store(_))));
}
