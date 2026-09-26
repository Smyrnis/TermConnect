use std::path::Path;

use super::*;

fn object(bucket: &str, key: &str) -> Location {
    Location::Object { bucket: bucket.to_string(), key: key.to_string() }
}

#[test]
fn without_a_configured_bucket_the_root_lists_buckets() {
    assert_eq!(locate(Path::new("/"), None), Location::Root);
    assert_eq!(locate(Path::new("/photos"), None), Location::Bucket("photos".to_string()));
    assert_eq!(locate(Path::new("/photos/2026/a.jpg"), None), object("photos", "2026/a.jpg"));
}

#[test]
fn a_configured_bucket_is_the_root() {
    assert_eq!(locate(Path::new("/"), Some("photos")), Location::Bucket("photos".to_string()));
    assert_eq!(locate(Path::new("/2026/a.jpg"), Some("photos")), object("photos", "2026/a.jpg"));
}

#[test]
fn parent_segments_never_leave_the_root() {
    assert_eq!(locate(Path::new("/../x"), Some("b")), object("b", "x"));
    assert_eq!(locate(Path::new("/b/c/../../d/e"), None), object("d", "e"));
}

#[test]
fn folder_prefixes_end_with_a_slash() {
    assert_eq!(Location::Bucket("b".to_string()).prefix(), "");
    assert_eq!(object("b", "a/c").prefix(), "a/c/");
    assert_eq!(Location::Root.prefix(), "");
}

#[test]
fn keys_are_encoded_per_segment() {
    assert_eq!(encode_key("a b/c+d#e%f&g ü.txt"), "a%20b/c%2Bd%23e%25f%26g%20%C3%BC.txt");
    assert_eq!(encode_key("keep-._~/x"), "keep-._~/x");
}

#[test]
fn path_style_puts_the_bucket_in_the_path() {
    let endpoint = Endpoint::new(false, "192.168.1.10", 9000);

    assert_eq!(endpoint.host(Some("photos"), Addressing::Path), "192.168.1.10:9000");
    assert_eq!(endpoint.path(Some("photos"), "a b.jpg", Addressing::Path), "/photos/a%20b.jpg");
    assert_eq!(endpoint.path(Some("photos"), "", Addressing::Path), "/photos");
    assert_eq!(endpoint.path(None, "", Addressing::Path), "/");
    assert_eq!(
        endpoint.url("192.168.1.10:9000", "/photos", "list-type=2"),
        "http://192.168.1.10:9000/photos?list-type=2"
    );
}

#[test]
fn virtual_hosting_puts_the_bucket_in_the_host_and_omits_the_default_port() {
    let endpoint = Endpoint::new(true, "s3.amazonaws.com", 443);

    assert_eq!(endpoint.host(Some("photos"), Addressing::Virtual), "photos.s3.amazonaws.com");
    assert_eq!(endpoint.host(None, Addressing::Virtual), "s3.amazonaws.com");
    assert_eq!(endpoint.path(Some("photos"), "a.jpg", Addressing::Virtual), "/a.jpg");
    assert_eq!(endpoint.path(Some("photos"), "", Addressing::Virtual), "/");
    assert_eq!(endpoint.url("photos.s3.amazonaws.com", "/a.jpg", ""), "https://photos.s3.amazonaws.com/a.jpg");
}

#[test]
fn ipv6_hosts_are_bracketed() {
    assert_eq!(Endpoint::new(false, "::1", 9000).host(None, Addressing::Path), "[::1]:9000");
}
