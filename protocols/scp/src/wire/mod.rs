use anyhow::anyhow;
use porthmos_vfs::{ErrorKind, ProtocolError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) mode: u32,
    pub(crate) size: u64,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    Ok,
    Warning(String),
    Fatal(String),
}

fn invalid(line: &[u8]) -> ProtocolError {
    ProtocolError::new(
        ErrorKind::Other,
        anyhow!("unexpected reply from the server's scp: {}", String::from_utf8_lossy(line)),
    )
}

pub(crate) fn parse_header(line: &[u8]) -> Result<Header, ProtocolError> {
    let text = std::str::from_utf8(line).map_err(|_| invalid(line))?;
    let rest = text.strip_prefix('C').ok_or_else(|| invalid(line))?;
    let mut parts = rest.splitn(3, ' ');
    let mode = parts.next().and_then(|mode| u32::from_str_radix(mode, 8).ok()).ok_or_else(|| invalid(line))?;
    let size = parts.next().and_then(|size| size.parse().ok()).ok_or_else(|| invalid(line))?;
    let name = parts.next().filter(|name| !name.is_empty()).ok_or_else(|| invalid(line))?;
    Ok(Header { mode, size, name: name.to_string() })
}

pub(crate) fn parse_reply(bytes: &[u8]) -> Option<(Reply, usize)> {
    match bytes.first()? {
        0 => Some((Reply::Ok, 1)),
        kind @ (1 | 2) => {
            let end = bytes.iter().position(|byte| *byte == b'\n')?;
            let message = String::from_utf8_lossy(&bytes[1..end]).into_owned();
            let reply = if *kind == 1 { Reply::Warning(message) } else { Reply::Fatal(message) };
            Some((reply, end + 1))
        }
        _ => Some((Reply::Fatal(String::from_utf8_lossy(bytes).into_owned()), bytes.len())),
    }
}

pub(crate) fn sink_header(size: u64, name: &str) -> String {
    format!("C0644 {size} {}\n", name.replace(['\n', '/'], " "))
}

#[cfg(test)]
mod tests;
