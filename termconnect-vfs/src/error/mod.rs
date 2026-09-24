use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Connect,
    Auth,
    AuthRejected,
    SessionStart,
    Cancelled,
    NotFound,
    PermissionDenied,
    Other,
}

#[derive(Debug)]
pub struct ProtocolError {
    kind: ErrorKind,
    source: anyhow::Error,
}

impl ProtocolError {
    pub fn new(kind: ErrorKind, source: impl Into<anyhow::Error>) -> Self {
        Self { kind, source: source.into() }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl From<std::io::Error> for ProtocolError {
    fn from(error: std::io::Error) -> Self {
        let kind = match error.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
            _ => ErrorKind::Other,
        };
        Self::new(kind, error)
    }
}

impl From<anyhow::Error> for ProtocolError {
    fn from(error: anyhow::Error) -> Self {
        Self::new(ErrorKind::Other, error)
    }
}

#[cfg(test)]
mod tests;
