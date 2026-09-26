#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod certificates;
mod trust;

pub use certificates::{
    CertificateDetails, KnownCertificates, ProblemSlot, TrustProblem, TrustVerifier, client_config, details,
};
pub use trust::ask_to_trust;
