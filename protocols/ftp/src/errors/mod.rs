use anyhow::anyhow;
use porthmos_vfs::{ErrorKind, ProtocolError};
use suppaftp::FtpError;

const SESSION_REUSE: &str = "the server requires TLS session reuse on data connections";

fn is_missing(text: &str) -> bool {
    ["no such", "not found", "doesn't exist", "does not exist"].iter().any(|hint| text.contains(hint))
}

pub(crate) fn reply_error(code: u32, text: &str) -> ProtocolError {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let kind = match code {
        550 if is_missing(&lower) => ErrorKind::NotFound,
        553 => ErrorKind::PermissionDenied,
        550 if lower.contains("permission") || lower.contains("denied") => ErrorKind::PermissionDenied,
        _ => ErrorKind::Other,
    };
    if code == 522 && lower.contains("reuse") {
        return ProtocolError::new(kind, anyhow!("{code} {text} ({SESSION_REUSE})"));
    }
    ProtocolError::new(kind, anyhow!("{code} {text}"))
}

pub(crate) fn ftp_error(err: FtpError) -> ProtocolError {
    match err {
        FtpError::UnexpectedResponse(response) => {
            reply_error(response.status.code(), &String::from_utf8_lossy(&response.body))
        }
        other => ProtocolError::new(ErrorKind::Other, anyhow!(other)),
    }
}

#[cfg(test)]
mod tests;
