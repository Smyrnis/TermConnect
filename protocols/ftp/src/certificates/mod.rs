use std::{
    collections::BTreeMap,
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use porthmos_vfs::{ErrorKind, ProtocolError};
use rustls::{
    ClientConfig, DigitallySignedStruct, Error as TlsError, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::{CryptoProvider, aws_lc_rs, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CertificateDetails {
    pub(crate) fingerprint: String,
    pub(crate) subject: String,
    pub(crate) expires: String,
}

const UNKNOWN: &str = "unknown";

pub(crate) fn details(der: &[u8]) -> CertificateDetails {
    let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(der)));
    match x509_parser::parse_x509_certificate(der) {
        Ok((_, certificate)) => {
            let date = certificate.validity().not_after.to_datetime().date();
            CertificateDetails {
                fingerprint,
                subject: certificate.subject().to_string(),
                expires: format!("{:04}-{:02}-{:02}", date.year(), u8::from(date.month()), date.day()),
            }
        }
        Err(_) => CertificateDetails { fingerprint, subject: UNKNOWN.to_string(), expires: UNKNOWN.to_string() },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pinned {
    sha256: String,
    subject: String,
    expires: String,
}

#[derive(Debug, Clone)]
pub(crate) struct KnownCertificates {
    path: PathBuf,
}

fn store_error(path: &std::path::Path, err: impl fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorKind::Connect, anyhow!("invalid {}: {err}", path.display()))
}

impl KnownCertificates {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn load(&self) -> Result<BTreeMap<String, Pinned>, ProtocolError> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => toml::from_str(&contents).map_err(|err| store_error(&self.path, err)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(err) => Err(store_error(&self.path, err)),
        }
    }

    pub(crate) fn lookup(&self, host: &str, port: u16) -> Result<Option<String>, ProtocolError> {
        Ok(self.load()?.remove(&format!("{host}:{port}")).map(|pinned| pinned.sha256))
    }

    pub(crate) fn remember(&self, host: &str, port: u16, details: &CertificateDetails) -> Result<(), ProtocolError> {
        let mut pinned = self.load()?;
        pinned.insert(
            format!("{host}:{port}"),
            Pinned {
                sha256: details.fingerprint.clone(),
                subject: details.subject.clone(),
                expires: details.expires.clone(),
            },
        );
        let contents = toml::to_string(&pinned).map_err(|err| store_error(&self.path, err))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| store_error(&self.path, err))?;
        }
        let temporary = self.path.with_extension("toml.tmp");
        std::fs::write(&temporary, contents).map_err(|err| store_error(&self.path, err))?;
        std::fs::rename(&temporary, &self.path).map_err(|err| store_error(&self.path, err))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrustProblem {
    Unknown(CertificateDetails),
    Changed,
    Store(String),
}

pub(crate) type ProblemSlot = Arc<Mutex<Option<TrustProblem>>>;

pub(crate) struct TrustVerifier {
    host: String,
    port: u16,
    store: KnownCertificates,
    system: Arc<WebPkiServerVerifier>,
    provider: Arc<CryptoProvider>,
    problem: ProblemSlot,
}

impl fmt::Debug for TrustVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrustVerifier").field("host", &self.host).field("port", &self.port).finish()
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(aws_lc_rs::default_provider())
}

impl TrustVerifier {
    pub(crate) fn new(host: &str, port: u16, store: KnownCertificates) -> (Self, ProblemSlot) {
        let provider = provider();
        let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let system = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
            .build()
            .expect("the bundled web PKI roots are valid");
        let problem: ProblemSlot = Arc::default();
        let verifier = Self { host: host.to_string(), port, store, system, provider, problem: problem.clone() };
        (verifier, problem)
    }

    fn report(&self, problem: TrustProblem, message: &str) -> TlsError {
        *self.problem.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(problem);
        TlsError::General(message.to_string())
    }
}

impl ServerCertVerifier for TrustVerifier {
    fn verify_server_cert(
        &self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], server_name: &ServerName<'_>,
        ocsp_response: &[u8], now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        if self.system.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now).is_ok() {
            return Ok(ServerCertVerified::assertion());
        }
        let presented = details(end_entity.as_ref());
        match self.store.lookup(&self.host, self.port) {
            Ok(Some(pinned)) if pinned == presented.fingerprint => Ok(ServerCertVerified::assertion()),
            Ok(Some(_)) => Err(self.report(TrustProblem::Changed, "certificate changed")),
            Ok(None) => Err(self.report(TrustProblem::Unknown(presented), "certificate not trusted")),
            Err(err) => Err(self.report(TrustProblem::Store(err.to_string()), "certificate store unreadable")),
        }
    }

    fn verify_tls12_signature(
        &self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

pub(crate) fn client_config(host: &str, port: u16, store: KnownCertificates) -> (Arc<ClientConfig>, ProblemSlot) {
    let (verifier, problem) = TrustVerifier::new(host, port, store);
    let config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("aws-lc-rs supports the default TLS versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    (Arc::new(config), problem)
}

#[cfg(test)]
mod tests;
