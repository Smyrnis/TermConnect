use std::collections::BTreeMap;

use super::*;
use crate::profiles::ConnectionSource;

fn draft(port: &str) -> ProfileDraft {
    ProfileDraft {
        name: " prod ".to_string(),
        host: " server.example.com ".to_string(),
        port: port.to_string(),
        username: " deploy ".to_string(),
        password: String::new(),
    }
}

#[test]
fn a_valid_draft_is_trimmed_into_a_profile() {
    let profile = draft("2222").validate(None).unwrap();

    assert_eq!(
        (profile.name.as_str(), profile.host.as_str(), profile.port, profile.username.as_str()),
        ("prod", "server.example.com", Some(2222), "deploy")
    );
    assert_eq!(profile.password, None);
    assert_eq!(profile.protocol, "sftp");
}

#[test]
fn a_non_numeric_port_is_rejected() {
    assert_eq!(draft("abc").validate(None).unwrap_err(), "Port must be a number from 1-65535");
}

#[test]
fn port_zero_is_rejected() {
    assert_eq!(draft("0").validate(None).unwrap_err(), "Port must be a number from 1-65535");
}

#[test]
fn empty_required_fields_are_rejected_in_form_order() {
    let mut empty = draft("22");
    empty.name = " ".to_string();
    assert_eq!(empty.validate(None).unwrap_err(), "Name can't be empty");
    empty.name = "n".to_string();
    empty.host = String::new();
    assert_eq!(empty.validate(None).unwrap_err(), "Host can't be empty");
    empty.host = "h".to_string();
    empty.username = String::new();
    assert_eq!(empty.validate(None).unwrap_err(), "Username can't be empty");
}

#[test]
fn a_password_is_kept_verbatim() {
    let mut with_password = draft("22");
    with_password.password = " spaced ".to_string();

    assert_eq!(with_password.validate(None).unwrap().password.as_deref(), Some(" spaced "));
}

#[test]
fn editing_preserves_the_protocol_and_options_not_shown_in_the_form() {
    let original = ConnectionEntry {
        name: "prod".to_string(),
        protocol: "ftp".to_string(),
        host: "old".to_string(),
        port: 21,
        username: "u".to_string(),
        password: None,
        options: BTreeMap::from([("remote_path".to_string(), "/var/www".to_string())]),
        source: ConnectionSource::Profile,
    };

    let profile = draft("21").validate(Some(&original)).unwrap();

    assert_eq!(profile.protocol, "ftp");
    assert_eq!(profile.options.get("remote_path").map(String::as_str), Some("/var/www"));
}
