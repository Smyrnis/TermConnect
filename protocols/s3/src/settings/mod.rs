use porthmos_vfs::Target;

const DEFAULT_REGION: &str = "us-east-1";
const AWS_SUFFIX: &str = ".amazonaws.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Addressing {
    Path,
    Virtual,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct S3Settings {
    pub(crate) endpoint: String,
    pub(crate) port: u16,
    pub(crate) secure: bool,
    pub(crate) region: String,
    pub(crate) bucket: Option<String>,
    pub(crate) addressing: String,
    pub(crate) access_key: String,
    pub(crate) secret: Option<String>,
}

fn filled(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string)
}

struct Pasted {
    host: String,
    port: Option<u16>,
    secure: Option<bool>,
}

fn pasted(endpoint: &str) -> Pasted {
    let lower = endpoint.trim().to_ascii_lowercase();
    let (secure, rest) = match (lower.strip_prefix("https://"), lower.strip_prefix("http://")) {
        (Some(rest), _) => (Some(true), rest),
        (_, Some(rest)) => (Some(false), rest),
        _ => (None, lower.as_str()),
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        match bracketed.split_once(']') {
            Some((host, after)) => (host, after.strip_prefix(':').and_then(|port| port.parse().ok())),
            None => (bracketed, None),
        }
    } else if authority.matches(':').count() == 1 {
        let (host, port) = authority.split_once(':').unwrap_or((authority, ""));
        (host, port.parse().ok())
    } else {
        (authority, None)
    };
    Pasted { host: host.to_string(), port, secure }
}

fn dns_bucket(bucket: &str) -> bool {
    !bucket.is_empty()
        && bucket.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.')
}

fn ip_address(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

impl S3Settings {
    pub(crate) fn from_target(target: &Target) -> Self {
        let pasted = pasted(&target.host);
        Self {
            endpoint: pasted.host,
            port: pasted.port.unwrap_or(target.port),
            secure: pasted.secure.unwrap_or(target.option("security") != Some("http")),
            region: filled(target.option("region")).unwrap_or_else(|| DEFAULT_REGION.to_string()),
            bucket: filled(target.option("bucket")),
            addressing: filled(target.option("addressing")).unwrap_or_else(|| "auto".to_string()),
            access_key: target.username.trim().to_string(),
            secret: target.password.clone().filter(|secret| !secret.is_empty()),
        }
    }

    pub(crate) fn addressing_for(&self, bucket: &str) -> Addressing {
        let wanted = match self.addressing.as_str() {
            "path" => Addressing::Path,
            "virtual" => Addressing::Virtual,
            _ if self.endpoint.ends_with(AWS_SUFFIX) => Addressing::Virtual,
            _ => Addressing::Path,
        };
        let hostable = dns_bucket(bucket) && !(self.secure && bucket.contains('.')) && !ip_address(&self.endpoint);
        if wanted == Addressing::Virtual && hostable { Addressing::Virtual } else { Addressing::Path }
    }
}

#[cfg(test)]
mod tests;
