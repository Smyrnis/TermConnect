use hmac::{Hmac, Mac};
use percent_encoding::utf8_percent_encode;
use sha2::{Digest, Sha256};

use crate::locate::UNRESERVED;

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

pub(crate) struct Credentials {
    pub(crate) access_key: String,
    pub(crate) secret: String,
}

pub(crate) struct Signable<'a> {
    pub(crate) method: &'a str,
    pub(crate) path: &'a str,
    pub(crate) query: &'a [(String, String)],
    pub(crate) headers: &'a [(String, String)],
    pub(crate) payload_hash: &'a str,
}

pub(crate) fn payload_hash(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

pub(crate) fn encode_query_value(value: &str) -> String {
    utf8_percent_encode(value, UNRESERVED).to_string()
}

pub(crate) fn canonical_query(query: &[(String, String)]) -> String {
    let mut encoded: Vec<(String, String)> =
        query.iter().map(|(name, value)| (encode_query_value(name), encode_query_value(value))).collect();
    encoded.sort();
    encoded.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("&")
}

pub(crate) fn canonical_request(request: &Signable) -> (String, String) {
    let mut headers: Vec<(String, String)> =
        request.headers.iter().map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string())).collect();
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(name, value)| format!("{name}:{value}\n")).collect();
    let signed = headers.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(";");
    let canonical = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed}\n{}",
        request.method,
        request.path,
        canonical_query(request.query),
        request.payload_hash
    );
    (canonical, signed)
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

pub(crate) fn authorization(
    request: &Signable, credentials: &Credentials, region: &str, service: &str, amz_date: &str,
) -> String {
    let date = &amz_date[..amz_date.len().min(8)];
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let (canonical, signed) = canonical_request(request);
    let to_sign = format!("{ALGORITHM}\n{amz_date}\n{scope}\n{}", payload_hash(canonical.as_bytes()));
    let key = [date, region, service, "aws4_request"]
        .iter()
        .fold(format!("AWS4{}", credentials.secret).into_bytes(), |key, part| hmac(&key, part));
    let signature = hex::encode(hmac(&key, &to_sign));
    format!("{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}", credentials.access_key)
}

#[cfg(test)]
mod tests;
