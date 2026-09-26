use std::collections::BTreeMap;

use porthmos_vfs::Target;

use super::*;

fn target(host: &str, options: &[(&str, &str)]) -> Target {
    Target {
        name: "nas".to_string(),
        host: host.to_string(),
        port: 443,
        username: "alice".to_string(),
        password: Some("pw".to_string()),
        options: options.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn defaults_are_https_at_the_server_root() {
    let settings = WebDavSettings::from_target(&target("nas.local", &[]));

    assert!(settings.secure);
    assert_eq!(settings.root, "/");
    assert_eq!(settings.origin(), "https://nas.local:443");
    assert_eq!((settings.username.as_str(), settings.password.as_deref()), ("alice", Some("pw")));
}

#[test]
fn http_security_gives_an_http_origin() {
    let settings = WebDavSettings::from_target(&target("nas.local", &[("security", "http")]));

    assert!(!settings.secure);
    assert_eq!(settings.origin(), "http://nas.local:443");
}

#[test]
fn ipv6_hosts_are_bracketed_in_the_origin() {
    assert_eq!(WebDavSettings::from_target(&target("::1", &[])).origin(), "https://[::1]:443");
}

#[test]
fn an_empty_saved_password_counts_as_none() {
    let mut target = target("nas.local", &[]);
    target.password = Some(String::new());

    assert_eq!(WebDavSettings::from_target(&target).password, None);
}

#[test]
fn roots_are_normalised_to_leading_and_trailing_slashes() {
    assert_eq!(normalize_root("remote.php/dav/files/alice"), "/remote.php/dav/files/alice/");
    assert_eq!(normalize_root("//a//b/"), "/a/b/");
    assert_eq!(normalize_root(""), "/");
    assert_eq!(normalize_root(" / "), "/");
}
