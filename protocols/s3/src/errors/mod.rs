use std::{error::Error as _, path::Path};

use anyhow::anyhow;
use porthmos_vfs::{ErrorKind, ProtocolError};
use reqwest::StatusCode;

pub(crate) const NO_RESPONSE: &str = "the server did not respond in time";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct S3Error {
    pub(crate) status: u16,
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) region: Option<String>,
}

impl S3Error {
    pub(crate) fn describe(&self) -> String {
        if self.code.is_empty() {
            StatusCode::from_u16(self.status)
                .map(|status| status.to_string())
                .unwrap_or_else(|_| self.status.to_string())
        } else {
            format!("{}: {}", self.code, self.message)
        }
    }
}

pub(crate) fn to_protocol(error: &S3Error, path: &Path) -> ProtocolError {
    let shown = path.display();
    match (error.code.as_str(), error.status) {
        ("NoSuchKey" | "NoSuchBucket" | "NoSuchUpload", _) | ("", 404) => {
            ProtocolError::new(ErrorKind::NotFound, anyhow!("{shown} not found"))
        }
        ("AccessDenied", _) | ("", 403) => {
            ProtocolError::new(ErrorKind::PermissionDenied, anyhow!("{shown}: permission denied"))
        }
        ("SlowDown", _) | ("", 503) => {
            ProtocolError::new(ErrorKind::Other, anyhow!("the server is busy (SlowDown) \u{2014} try again"))
        }
        ("EntityTooLarge", _) => ProtocolError::new(ErrorKind::Other, anyhow!("{shown} is too large for this server")),
        _ => ProtocolError::new(ErrorKind::Other, anyhow!(error.describe())),
    }
}

pub(crate) enum Failure {
    Request(reqwest::Error),
    TimedOut,
}

impl From<reqwest::Error> for Failure {
    fn from(err: reqwest::Error) -> Self {
        Failure::Request(err)
    }
}

impl Failure {
    pub(crate) fn into_error(self, kind: ErrorKind) -> ProtocolError {
        match self {
            Failure::Request(err) => request_error(kind, err),
            Failure::TimedOut => ProtocolError::new(kind, anyhow!(NO_RESPONSE)),
        }
    }
}

pub(crate) fn request_error(kind: ErrorKind, err: reqwest::Error) -> ProtocolError {
    if err.is_timeout() {
        return ProtocolError::new(kind, anyhow!(NO_RESPONSE));
    }
    let err = err.without_url();
    let mut message = err.to_string();
    let mut cause = err.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    ProtocolError::new(kind, anyhow!(message))
}

#[cfg(test)]
mod tests;
