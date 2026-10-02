use std::path::{Component, Path};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::settings::Addressing;

pub(crate) const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Location {
    Root,
    Bucket(String),
    Object { bucket: String, key: String },
}

impl Location {
    pub(crate) fn prefix(&self) -> String {
        match self {
            Location::Object { key, .. } => format!("{key}/"),
            Location::Root | Location::Bucket(_) => String::new(),
        }
    }

    pub(crate) fn bucket(&self) -> Option<&str> {
        match self {
            Location::Root => None,
            Location::Bucket(bucket) | Location::Object { bucket, .. } => Some(bucket),
        }
    }
}

pub(crate) fn segments(path: &Path) -> Vec<String> {
    let mut segments = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => segments.push(name.to_string_lossy().into_owned()),
            Component::ParentDir => {
                segments.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    segments
}

pub(crate) fn locate(path: &Path, configured: Option<&str>) -> Location {
    let mut segments = segments(path);
    let bucket = match configured {
        Some(bucket) => bucket.to_string(),
        None if segments.is_empty() => return Location::Root,
        None => segments.remove(0),
    };
    if segments.is_empty() { Location::Bucket(bucket) } else { Location::Object { bucket, key: segments.join("/") } }
}

pub(crate) fn encode_key(key: &str) -> String {
    key.split('/').map(|segment| utf8_percent_encode(segment, UNRESERVED).to_string()).collect::<Vec<_>>().join("/")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    secure: bool,
    host: String,
    port: u16,
}

impl Endpoint {
    pub(crate) fn new(secure: bool, host: &str, port: u16) -> Self {
        let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
        Self { secure, host, port }
    }

    fn default_port(&self) -> u16 {
        if self.secure { 443 } else { 80 }
    }

    pub(crate) fn host(&self, bucket: Option<&str>, addressing: Addressing) -> String {
        let name = match (bucket, addressing) {
            (Some(bucket), Addressing::Virtual) => format!("{bucket}.{}", self.host),
            _ => self.host.clone(),
        };
        if self.port == self.default_port() { name } else { format!("{name}:{}", self.port) }
    }

    pub(crate) fn path(&self, bucket: Option<&str>, key: &str, addressing: Addressing) -> String {
        match (bucket, addressing) {
            (Some(bucket), Addressing::Path) if key.is_empty() => format!("/{}", encode_key(bucket)),
            (Some(bucket), Addressing::Path) => format!("/{}/{}", encode_key(bucket), encode_key(key)),
            _ => format!("/{}", encode_key(key)),
        }
    }

    pub(crate) fn url(&self, host: &str, path: &str, query: &str) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        if query.is_empty() { format!("{scheme}://{host}{path}") } else { format!("{scheme}://{host}{path}?{query}") }
    }
}

#[cfg(test)]
mod tests;
