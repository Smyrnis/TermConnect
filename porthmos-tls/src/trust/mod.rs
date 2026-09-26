use anyhow::anyhow;
use porthmos_vfs::{Answer, ErrorKind, Prompter, ProtocolError, Question};

use crate::{KnownCertificates, TrustProblem};

pub async fn ask_to_trust(
    problem: TrustProblem, name: &str, host: &str, port: u16, store: &KnownCertificates, prompter: &mut dyn Prompter,
) -> Result<(), ProtocolError> {
    match problem {
        TrustProblem::Unknown(details) => {
            let question = Question::TrustCertificate {
                name: name.to_string(),
                host: host.to_string(),
                port,
                fingerprint: details.fingerprint.clone(),
                subject: details.subject.clone(),
                expires: details.expires.clone(),
            };
            if !matches!(prompter.ask(question).await, Some(Answer::Confirmed)) {
                return Err(ProtocolError::new(ErrorKind::Cancelled, anyhow!("Connection cancelled")));
            }
            store.remember(host, port, &details)
        }
        TrustProblem::Changed => Err(ProtocolError::new(
            ErrorKind::Connect,
            anyhow!(
                "CERTIFICATE CHANGED for {host}:{port} \u{2014} refusing to connect (see {})",
                store.path().display()
            ),
        )),
        TrustProblem::Store(message) => Err(ProtocolError::new(ErrorKind::Connect, anyhow!(message))),
    }
}

#[cfg(test)]
mod tests;
