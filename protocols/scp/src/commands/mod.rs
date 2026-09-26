use std::path::Path;

use anyhow::anyhow;
use porthmos_ssh::shell_quote;
use porthmos_vfs::{ErrorKind, ProtocolError, path_to_remote_string};

pub(crate) const PROBE: &str =
    "printf 'porthmos\\n%s\\n' \"$HOME\"; if command -v scp >/dev/null 2>&1; then echo scp; fi";
pub(crate) const PROBE_MARKER: &str = "porthmos";
const LISTING_ENVIRONMENT: &str = "TZ=UTC0 LC_ALL=C QUOTING_STYLE=literal TIME_STYLE=locale";

fn quoted(path: &Path) -> String {
    shell_quote(&path_to_remote_string(path))
}

pub(crate) fn list(dir: &Path) -> String {
    let mut text = path_to_remote_string(dir);
    if !text.ends_with('/') {
        text.push('/');
    }
    format!("{LISTING_ENVIRONMENT} ls -lanL -- {}", shell_quote(&text))
}

pub(crate) fn stat(path: &Path) -> String {
    format!("{LISTING_ENVIRONMENT} ls -ldnL -- {}", quoted(path))
}

pub(crate) fn mkdir(path: &Path) -> String {
    format!("mkdir -- {}", quoted(path))
}

pub(crate) fn remove(path: &Path) -> String {
    format!("rm -- {}", quoted(path))
}

pub(crate) fn remove_tree(path: &Path) -> String {
    format!("rm -rf -- {}", quoted(path))
}

pub(crate) fn rename(from: &Path, to: &Path) -> String {
    format!("mv -f -- {} {}", quoted(from), quoted(to))
}

pub(crate) fn read_from(path: &Path, offset: u64) -> String {
    if offset == 0 {
        format!("cat -- {}", quoted(path))
    } else {
        format!("tail -c +{} -- {}", offset + 1, quoted(path))
    }
}

pub(crate) fn scp_source(path: &Path) -> String {
    format!("scp -f -- {}", quoted(path))
}

pub(crate) fn scp_sink(path: &Path) -> String {
    format!(": > {} && scp -t -- {}", quoted(path), quoted(path))
}

pub(crate) fn append(path: &Path) -> String {
    format!("cat >> {}", quoted(path))
}

pub(crate) fn create(path: &Path) -> String {
    format!("cat > {}", quoted(path))
}

pub(crate) fn failure(stderr: &[u8], status: Option<u32>, path: &Path, command: &str) -> ProtocolError {
    let text = String::from_utf8_lossy(stderr);
    let shown = path.display();
    if ["No such file or directory", "Directory nonexistent", "nonexistent directory"]
        .iter()
        .any(|hint| text.contains(hint))
    {
        return ProtocolError::new(ErrorKind::NotFound, anyhow!("{shown} not found"));
    }
    if ["Permission denied", "Operation not permitted", "Read-only file system"].iter().any(|hint| text.contains(hint))
    {
        return ProtocolError::new(ErrorKind::PermissionDenied, anyhow!("{shown}: permission denied"));
    }
    if text.contains("File exists") {
        return ProtocolError::new(ErrorKind::Other, anyhow!("{shown} already exists"));
    }
    match text.lines().map(str::trim).find(|line| !line.is_empty()) {
        Some(first) => ProtocolError::new(ErrorKind::Other, anyhow!(first.to_string())),
        None => {
            let exit = status.map_or_else(|| "no status".to_string(), |code| code.to_string());
            ProtocolError::new(ErrorKind::Other, anyhow!("{command} failed (exit {exit})"))
        }
    }
}

#[cfg(test)]
mod tests;
