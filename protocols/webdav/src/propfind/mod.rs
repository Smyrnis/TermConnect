use anyhow::anyhow;
use porthmos_vfs::{ErrorKind, FileKind, Metadata, ProtocolError};
use quick_xml::{
    NsReader,
    events::Event,
    name::{Namespace, ResolveResult},
};

pub(crate) const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/><D:getcontentlength/><D:getlastmodified/></D:prop></D:propfind>"#;
pub(crate) const XML_CONTENT_TYPE: &str = "application/xml; charset=utf-8";

const DAV: Namespace<'static> = Namespace("DAV:");

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Resource {
    pub(crate) href: String,
    pub(crate) status: Option<u16>,
    pub(crate) collection: bool,
    pub(crate) size: Option<u64>,
    pub(crate) modified: Option<u64>,
}

impl Resource {
    pub(crate) fn metadata(&self) -> Metadata {
        Metadata {
            size: self.size.unwrap_or(0),
            modified: self.modified,
            kind: if self.collection { FileKind::Dir } else { FileKind::File },
            permissions: None,
        }
    }
}

#[derive(Default)]
struct Found {
    collection: bool,
    size: Option<u64>,
    modified: Option<u64>,
    status: Option<u16>,
}

fn invalid(message: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorKind::Other, anyhow!("invalid PROPFIND response: {message}"))
}

fn status_code(line: &str) -> Option<u16> {
    line.split_whitespace().nth(1)?.parse().ok()
}

fn modified_seconds(text: &str) -> Option<u64> {
    let time = httpdate::parse_http_date(text.trim()).ok()?;
    time.duration_since(std::time::UNIX_EPOCH).ok().map(|elapsed| elapsed.as_secs())
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

pub(crate) fn parse_multistatus(xml: &str) -> Result<Vec<Resource>, ProtocolError> {
    let mut reader = NsReader::from_str(xml);
    let mut resources = Vec::new();
    let mut open: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut current = Resource::default();
    let mut found = Found::default();
    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(invalid)?;
        let in_dav = matches!(namespace, ResolveResult::Bound(bound) if bound == DAV);
        match event {
            Event::Start(element) => {
                let name = if in_dav { element.local_name().as_ref().to_string() } else { String::new() };
                if name == "response" {
                    current = Resource::default();
                }
                if name == "propstat" {
                    found = Found::default();
                }
                if name == "collection" && open.last().is_some_and(|parent| parent == "resourcetype") {
                    found.collection = true;
                }
                open.push(name);
                text.clear();
            }
            Event::Empty(element)
                if in_dav
                    && element.local_name().as_ref() == "collection"
                    && open.last().is_some_and(|parent| parent == "resourcetype") =>
            {
                found.collection = true;
            }
            Event::Text(content) => text.push_str(&content.xml10_content()),
            Event::CData(content) => text.push_str(&content.xml10_content()),
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref().map_err(invalid)? {
                    Some(character) => Some(character),
                    None => entity(&reference.into_inner()),
                };
                text.extend(resolved);
            }
            Event::End(_) => {
                let name = open.pop().unwrap_or_default();
                let parent = open.last().map(String::as_str).unwrap_or_default();
                match (parent, name.as_str()) {
                    ("response", "href") => current.href = text.trim().to_string(),
                    ("response", "status") => current.status = status_code(&text),
                    ("propstat", "status") => found.status = status_code(&text),
                    ("prop", "getcontentlength") => found.size = text.trim().parse().ok(),
                    ("prop", "getlastmodified") => found.modified = modified_seconds(&text),
                    ("response", "propstat") if found.status == Some(200) => {
                        current.collection |= found.collection;
                        current.size = current.size.or(found.size);
                        current.modified = current.modified.or(found.modified);
                    }
                    ("multistatus", "response") => resources.push(std::mem::take(&mut current)),
                    _ => {}
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if open.is_empty() { Ok(resources) } else { Err(invalid("unexpected end of document")) }
}

#[cfg(test)]
mod tests;
