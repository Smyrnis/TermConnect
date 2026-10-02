use std::collections::BTreeMap;

use porthmos_vfs::Target;

use super::*;

fn target(host: &str, options: &[(&str, &str)]) -> Target {
    Target {
        name: "store".to_string(),
        host: host.to_string(),
        port: 443,
        username: " AKID ".to_string(),
        password: Some("secret".to_string()),
        options: options.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn defaults_are_https_in_us_east_1_listing_buckets() {
    let settings = S3Settings::from_target(&target("s3.amazonaws.com", &[]));

    assert!(settings.secure);
    assert_eq!(settings.region, "us-east-1");
    assert_eq!(settings.bucket, None);
    assert_eq!((settings.access_key.as_str(), settings.secret.as_deref()), ("AKID", Some("secret")));
}

#[test]
fn options_are_read_and_trimmed() {
    let settings = S3Settings::from_target(&target(
        "minio.local",
        &[("security", "http"), ("region", " eu-west-1 "), ("bucket", " photos "), ("addressing", "path")],
    ));

    assert!(!settings.secure);
    assert_eq!((settings.region.as_str(), settings.bucket.as_deref()), ("eu-west-1", Some("photos")));
}

#[test]
fn empty_bucket_region_and_secret_fall_back() {
    let mut target = target("h", &[("bucket", "  "), ("region", "")]);
    target.password = Some(String::new());
    let settings = S3Settings::from_target(&target);

    assert_eq!((settings.bucket, settings.secret, settings.region.as_str()), (None, None, "us-east-1"));
}

#[test]
fn automatic_addressing_is_virtual_only_for_aws() {
    assert_eq!(S3Settings::from_target(&target("s3.amazonaws.com", &[])).addressing_for("photos"), Addressing::Virtual);
    assert_eq!(
        S3Settings::from_target(&target("s3.eu-west-1.amazonaws.com", &[])).addressing_for("photos"),
        Addressing::Virtual
    );
    assert_eq!(S3Settings::from_target(&target("minio.local", &[])).addressing_for("photos"), Addressing::Path);
}

#[test]
fn virtual_addressing_falls_back_to_path_when_the_bucket_cannot_be_a_host() {
    let virtual_choice = |host: &str| S3Settings::from_target(&target(host, &[("addressing", "virtual")]));

    assert_eq!(virtual_choice("r2.example.com").addressing_for("photos"), Addressing::Virtual);
    assert_eq!(virtual_choice("192.168.1.10").addressing_for("photos"), Addressing::Path);
    assert_eq!(virtual_choice("s3.amazonaws.com").addressing_for("my.photos"), Addressing::Path);
    assert_eq!(virtual_choice("s3.amazonaws.com").addressing_for("Photos_Old"), Addressing::Path);
}

#[test]
fn path_addressing_is_always_path() {
    assert_eq!(
        S3Settings::from_target(&target("s3.amazonaws.com", &[("addressing", "path")])).addressing_for("photos"),
        Addressing::Path
    );
}

#[test]
fn pasted_endpoints_are_reduced_to_a_lowercase_host() {
    assert_eq!(
        S3Settings::from_target(&target("https://ACCT.r2.cloudflarestorage.com/", &[])).endpoint,
        "acct.r2.cloudflarestorage.com"
    );
    assert_eq!(S3Settings::from_target(&target("http://minio.local/bucket/path", &[])).endpoint, "minio.local");
    assert_eq!(S3Settings::from_target(&target(" S3.AmazonAWS.com ", &[])).endpoint, "s3.amazonaws.com");
    assert_eq!(S3Settings::from_target(&target("::1", &[])).endpoint, "::1");
    assert_eq!(S3Settings::from_target(&target("https://user@host.example/", &[])).endpoint, "host.example");
}

#[test]
fn a_pasted_port_and_scheme_are_used() {
    let minio = S3Settings::from_target(&target("http://minio.local:9000", &[]));
    let ipv6 = S3Settings::from_target(&target("https://[::1]:9443/", &[("security", "http")]));

    assert_eq!((minio.endpoint.as_str(), minio.port, minio.secure), ("minio.local", 9000, false));
    assert_eq!((ipv6.endpoint.as_str(), ipv6.port, ipv6.secure), ("::1", 9443, true));
    assert_eq!(S3Settings::from_target(&target("minio.local:9000", &[])).port, 9000);
}
