use std::path::{Component, Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Locator {
    origin: String,
    root: Vec<String>,
}

impl Locator {
    pub(crate) fn new(origin: &str, root: &str) -> Self {
        let root = root
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
            .collect();
        Self { origin: origin.to_string(), root }
    }

    pub(crate) fn href(&self, path: &Path, collection: bool) -> String {
        let encoded: Vec<String> = self
            .root
            .iter()
            .cloned()
            .chain(segments(path))
            .map(|segment| utf8_percent_encode(&segment, SEGMENT).to_string())
            .collect();
        let mut href = format!("/{}", encoded.join("/"));
        if collection && !href.ends_with('/') {
            href.push('/');
        }
        href
    }

    pub(crate) fn root(&self) -> String {
        if self.root.is_empty() { "/".to_string() } else { format!("/{}/", self.root.join("/")) }
    }

    pub(crate) fn url(&self, href: &str) -> String {
        format!("{}{href}", self.origin)
    }

    pub(crate) fn path_of_href(&self, href: &str) -> Option<PathBuf> {
        let absolute = match href.find("://") {
            Some(scheme_end) => {
                let rest = &href[scheme_end + 3..];
                rest.find('/').map_or("/", |path_start| &rest[path_start..])
            }
            None => href,
        };
        let without_query = absolute.split(['?', '#']).next().unwrap_or("/");
        let decoded: Vec<String> = without_query
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
            .collect();
        let inside = decoded.strip_prefix(self.root.as_slice())?;
        Some(to_path(inside))
    }
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    to_path(&segments(path))
}

fn segments(path: &Path) -> Vec<String> {
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

fn to_path(segments: &[String]) -> PathBuf {
    let mut path = PathBuf::from("/");
    path.extend(segments);
    path
}

#[cfg(test)]
mod tests;
