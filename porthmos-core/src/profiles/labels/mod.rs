use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Labels {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_keyring: Vec<String>,
}

impl Labels {
    pub fn is_empty(&self) -> bool {
        self.group.is_none() && self.tags.is_empty() && self.in_keyring.is_empty()
    }
}

pub fn normalize_group(raw: &str) -> Option<String> {
    let segments: Vec<&str> = raw.split('/').map(str::trim).filter(|segment| !segment.is_empty()).collect();
    if segments.is_empty() { None } else { Some(segments.join("/")) }
}

pub fn normalize_tags<I, S>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut tags: Vec<String> = Vec::new();
    for tag in raw {
        let tag = tag.as_ref().trim();
        let tag = tag.strip_prefix('#').unwrap_or(tag).trim();
        if !tag.is_empty() && !tags.iter().any(|kept| kept.to_lowercase() == tag.to_lowercase()) {
            tags.push(tag.to_string());
        }
    }
    tags
}

pub fn parse_tags(raw: &str) -> Vec<String> {
    normalize_tags(raw.split(','))
}

#[cfg(test)]
mod tests;
