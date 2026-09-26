use anyhow::anyhow;
use porthmos_vfs::{ErrorKind, ProtocolError};
use quick_xml::{Reader, events::Event};

use crate::clock::parse_iso8601;
pub(crate) use crate::errors::S3Error;

const NAMESPACE: &str = "http://s3.amazonaws.com/doc/2006-03-01/";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Node {
    name: String,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.name == name)
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    fn text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|child| child.text.as_str())
    }

    fn truncated(&self) -> bool {
        self.text_of("IsTruncated").is_some_and(|text| text.trim() == "true")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bucket {
    pub(crate) name: String,
    pub(crate) created: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Object {
    pub(crate) key: String,
    pub(crate) size: u64,
    pub(crate) modified: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Listing {
    pub(crate) objects: Vec<Object>,
    pub(crate) prefixes: Vec<String>,
    pub(crate) next: Option<String>,
    pub(crate) truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cursor {
    Token(String),
    StartAfter(String),
}

impl Listing {
    pub(crate) fn cursor(&self, previous: Option<&Cursor>) -> Result<Option<Cursor>, ProtocolError> {
        if !self.truncated {
            return Ok(None);
        }
        let last_key =
            self.objects.iter().map(|object| object.key.as_str()).chain(self.prefixes.iter().map(String::as_str)).max();
        let next = match (&self.next, last_key) {
            (Some(token), _) => Cursor::Token(token.clone()),
            (None, Some(key)) if self.prefixes.iter().any(|prefix| prefix == key) => {
                Cursor::StartAfter(format!("{}0", key.trim_end_matches('/')))
            }
            (None, Some(key)) => Cursor::StartAfter(key.to_string()),
            (None, None) => return Err(invalid("a truncated listing gave no place to continue")),
        };
        if previous == Some(&next) {
            return Err(invalid("the server repeated a listing page"));
        }
        Ok(Some(next))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Upload {
    pub(crate) key: String,
    pub(crate) id: String,
    pub(crate) initiated: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Uploads {
    pub(crate) uploads: Vec<Upload>,
    pub(crate) prefixes: Vec<String>,
    pub(crate) next: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part {
    pub(crate) number: u32,
    pub(crate) size: u64,
    pub(crate) etag: String,
    pub(crate) modified: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Parts {
    pub(crate) parts: Vec<Part>,
    pub(crate) next: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeleteFailure {
    pub(crate) key: String,
    pub(crate) code: String,
}

fn invalid(message: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::new(ErrorKind::Other, anyhow!("invalid S3 response: {message}"))
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

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_string()
}

fn parse(xml: &str) -> Result<Node, ProtocolError> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        match reader.read_event().map_err(invalid)? {
            Event::Start(element) => {
                stack.push(Node { name: local(element.name().as_ref()), ..Node::default() });
            }
            Event::Empty(element) => {
                let node = Node { name: local(element.name().as_ref()), ..Node::default() };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| invalid("unbalanced document"))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Err(invalid("unbalanced document")),
                }
            }
            Event::Text(content) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&content.xml10_content());
                }
            }
            Event::CData(content) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&content.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref().map_err(invalid)? {
                    Some(character) => Some(character),
                    None => entity(&reference.into_inner()),
                };
                if let Some(node) = stack.last_mut() {
                    node.text.extend(resolved);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let document = stack.pop().filter(|_| stack.is_empty()).ok_or_else(|| invalid("unexpected end of document"))?;
    document.children.into_iter().next().ok_or_else(|| invalid("empty document"))
}

fn root(xml: &str, expected: &str) -> Result<Node, ProtocolError> {
    let node = parse(xml)?;
    if node.name == expected { Ok(node) } else { Err(invalid(format!("expected {expected}, got {}", node.name))) }
}

fn number<T: std::str::FromStr>(node: &Node, name: &str) -> Option<T> {
    node.text_of(name)?.trim().parse().ok()
}

fn time(node: &Node, name: &str) -> Option<u64> {
    node.text_of(name).and_then(parse_iso8601)
}

fn prefixes(node: &Node) -> Vec<String> {
    node.all("CommonPrefixes").filter_map(|prefix| prefix.text_of("Prefix")).map(str::to_string).collect()
}

pub(crate) fn buckets(xml: &str) -> Result<Vec<Bucket>, ProtocolError> {
    let document = root(xml, "ListAllMyBucketsResult")?;
    let Some(list) = document.child("Buckets") else {
        return Ok(Vec::new());
    };
    Ok(list
        .all("Bucket")
        .filter_map(|bucket| {
            Some(Bucket { name: bucket.text_of("Name")?.to_string(), created: time(bucket, "CreationDate") })
        })
        .collect())
}

pub(crate) fn listing(xml: &str) -> Result<Listing, ProtocolError> {
    let document = root(xml, "ListBucketResult")?;
    let objects = document
        .all("Contents")
        .filter_map(|content| {
            Some(Object {
                key: content.text_of("Key")?.to_string(),
                size: number(content, "Size").unwrap_or(0),
                modified: time(content, "LastModified"),
            })
        })
        .collect();
    let truncated = document.truncated();
    let next = truncated.then(|| document.text_of("NextContinuationToken").map(str::to_string)).flatten();
    Ok(Listing { objects, prefixes: prefixes(&document), next, truncated })
}

pub(crate) fn uploads(xml: &str) -> Result<Uploads, ProtocolError> {
    let document = root(xml, "ListMultipartUploadsResult")?;
    let uploads = document
        .all("Upload")
        .filter_map(|upload| {
            Some(Upload {
                key: upload.text_of("Key")?.to_string(),
                id: upload.text_of("UploadId")?.to_string(),
                initiated: time(upload, "Initiated"),
            })
        })
        .collect();
    let next = if document.truncated() {
        Some((
            document.text_of("NextKeyMarker").unwrap_or_default().to_string(),
            document.text_of("NextUploadIdMarker").unwrap_or_default().to_string(),
        ))
    } else {
        None
    };
    Ok(Uploads { uploads, prefixes: prefixes(&document), next })
}

pub(crate) fn parts(xml: &str) -> Result<Parts, ProtocolError> {
    let document = root(xml, "ListPartsResult")?;
    let parts: Vec<Part> = document
        .all("Part")
        .filter_map(|part| {
            Some(Part {
                number: number(part, "PartNumber")?,
                size: number(part, "Size").unwrap_or(0),
                etag: part.text_of("ETag").unwrap_or_default().to_string(),
                modified: time(part, "LastModified"),
            })
        })
        .collect();
    let highest = parts.iter().map(|part: &Part| part.number).max();
    let next = if document.truncated() { number(&document, "NextPartNumberMarker").or(highest) } else { None };
    Ok(Parts { parts, next })
}

pub(crate) fn copy_part_etag(xml: &str) -> Option<String> {
    let document = parse(xml).ok().filter(|node| node.name == "CopyPartResult")?;
    document.text_of("ETag").map(str::to_string)
}

pub(crate) fn upload_id(xml: &str) -> Result<String, ProtocolError> {
    let document = root(xml, "InitiateMultipartUploadResult")?;
    document.text_of("UploadId").map(str::to_string).ok_or_else(|| invalid("no UploadId"))
}

pub(crate) fn delete_failures(xml: &str) -> Result<Vec<DeleteFailure>, ProtocolError> {
    let document = root(xml, "DeleteResult")?;
    Ok(document
        .all("Error")
        .map(|failure| DeleteFailure {
            key: failure.text_of("Key").unwrap_or_default().to_string(),
            code: failure.text_of("Code").unwrap_or_default().to_string(),
        })
        .collect())
}

fn error_from(status: u16, node: &Node) -> S3Error {
    S3Error {
        status,
        code: node.text_of("Code").unwrap_or_default().to_string(),
        message: node.text_of("Message").unwrap_or_default().to_string(),
        region: node.text_of("Region").map(str::to_string),
    }
}

pub(crate) fn error(status: u16, xml: &str) -> S3Error {
    match parse(xml) {
        Ok(node) if node.name == "Error" => error_from(status, &node),
        _ => S3Error { status, ..S3Error::default() },
    }
}

pub(crate) fn completion_error(xml: &str) -> Option<S3Error> {
    parse(xml).ok().filter(|node| node.name == "Error").map(|node| error_from(200, &node))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

pub(crate) fn complete_body(parts: &[(u32, String)]) -> String {
    let parts: String = parts
        .iter()
        .map(|(number, etag)| format!("<Part><PartNumber>{number}</PartNumber><ETag>{}</ETag></Part>", escape(etag)))
        .collect();
    format!("<CompleteMultipartUpload xmlns=\"{NAMESPACE}\">{parts}</CompleteMultipartUpload>")
}

pub(crate) fn delete_body(keys: &[String]) -> String {
    let objects: String = keys.iter().map(|key| format!("<Object><Key>{}</Key></Object>", escape(key))).collect();
    format!("<Delete xmlns=\"{NAMESPACE}\"><Quiet>true</Quiet>{objects}</Delete>")
}

#[cfg(test)]
mod tests;
