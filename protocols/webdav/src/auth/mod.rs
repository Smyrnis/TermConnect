use std::borrow::Cow;

use base64::{Engine, engine::general_purpose::STANDARD};
use digest_auth::{AlgorithmType, AuthContext, HttpMethod, WwwAuthenticateHeader};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Challenge {
    Basic,
    Digest(WwwAuthenticateHeader),
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Scheme {
    Basic,
    Digest(WwwAuthenticateHeader),
}

fn split_items(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut quoted = false;
    for character in value.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                item.push(character);
            }
            ',' if !quoted => items.push(std::mem::take(&mut item)),
            _ => item.push(character),
        }
    }
    items.push(item);
    items.into_iter().map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect()
}

fn challenge(scheme: &str, params: &[String]) -> Challenge {
    match scheme.to_ascii_lowercase().as_str() {
        "basic" => Challenge::Basic,
        "digest" => match digest_auth::parse(&params.join(", ")) {
            Ok(header) => Challenge::Digest(header),
            Err(_) => Challenge::Unsupported(scheme.to_string()),
        },
        _ => Challenge::Unsupported(scheme.to_string()),
    }
}

pub(crate) fn challenges<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<Challenge> {
    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    for value in values {
        for item in split_items(value) {
            let (head, rest) = item.split_once(' ').unwrap_or((item.as_str(), ""));
            if head.contains('=') {
                if let Some((_, params)) = grouped.last_mut() {
                    params.push(item.clone());
                }
                continue;
            }
            let params = if rest.trim().is_empty() { Vec::new() } else { vec![rest.trim().to_string()] };
            grouped.push((head.to_string(), params));
        }
    }
    grouped.iter().map(|(scheme, params)| challenge(scheme, params)).collect()
}

fn rank(challenge: &Challenge) -> u8 {
    match challenge {
        Challenge::Digest(header) if header.algorithm.algo != AlgorithmType::MD5 => 3,
        Challenge::Digest(_) => 2,
        Challenge::Basic => 1,
        Challenge::Unsupported(_) => 0,
    }
}

pub(crate) fn strongest(challenges: Vec<Challenge>) -> Result<Scheme, Vec<String>> {
    let unsupported = challenges
        .iter()
        .filter_map(|challenge| match challenge {
            Challenge::Unsupported(name) => Some(name.clone()),
            _ => None,
        })
        .collect();
    match challenges.into_iter().filter(|challenge| rank(challenge) > 0).max_by_key(rank) {
        Some(Challenge::Basic) => Ok(Scheme::Basic),
        Some(Challenge::Digest(header)) => Ok(Scheme::Digest(header)),
        _ => Err(unsupported),
    }
}

struct Credentials {
    username: String,
    password: String,
}

#[derive(Default)]
pub(crate) struct Authenticator {
    credentials: Option<Credentials>,
    scheme: Option<Scheme>,
}

impl Authenticator {
    pub(crate) fn use_scheme(&mut self, scheme: Scheme) {
        self.scheme = Some(scheme);
    }

    pub(crate) fn awaits_challenge(&self) -> bool {
        self.credentials.is_some() && self.scheme.is_none()
    }

    pub(crate) fn use_credentials(&mut self, username: &str, password: &str) {
        self.credentials = Some(Credentials { username: username.to_string(), password: password.to_string() });
    }

    pub(crate) fn header(&mut self, method: &str, uri: &str) -> Option<String> {
        self.header_with_cnonce(method, uri, None)
    }

    pub(crate) fn header_with_cnonce(&mut self, method: &str, uri: &str, cnonce: Option<&str>) -> Option<String> {
        let credentials = self.credentials.as_ref()?;
        match self.scheme.as_mut()? {
            Scheme::Basic => {
                Some(format!("Basic {}", STANDARD.encode(format!("{}:{}", credentials.username, credentials.password))))
            }
            Scheme::Digest(challenge) => {
                let mut context = AuthContext::new_with_method(
                    credentials.username.as_str(),
                    credentials.password.as_str(),
                    uri,
                    Option::<&[u8]>::None,
                    HttpMethod(Cow::Owned(method.to_string())),
                );
                if let Some(cnonce) = cnonce {
                    context.set_custom_cnonce(cnonce.to_string());
                }
                challenge.respond(&context).ok().map(|header| header.to_header_string())
            }
        }
    }

    pub(crate) fn refresh(&mut self, challenges: Vec<Challenge>) -> bool {
        if self.scheme.is_none() {
            if self.credentials.is_none() {
                return false;
            }
            let Ok(scheme) = strongest(challenges) else {
                return false;
            };
            self.scheme = Some(scheme);
            return true;
        }
        if !matches!(self.scheme, Some(Scheme::Digest(_))) {
            return false;
        }
        let stale = challenges.into_iter().find_map(|challenge| match challenge {
            Challenge::Digest(header) if header.stale => Some(header),
            _ => None,
        });
        match stale {
            Some(header) => {
                if let Some(Scheme::Digest(current)) = &self.scheme
                    && current.nonce == header.nonce
                {
                    return true;
                }
                self.scheme = Some(Scheme::Digest(header));
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests;
