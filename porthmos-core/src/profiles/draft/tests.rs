use std::collections::BTreeMap;

use porthmos_vfs::{Choice, CommonField, ConnectionForm, OptionField, OptionKind};

use super::*;
use crate::profiles::ConnectionSource;

const SECURITY: &[Choice] =
    &[Choice { value: "none", label: "None" }, Choice { value: "explicit", label: "Explicit TLS" }];

fn form() -> ConnectionForm {
    let mut form = ConnectionForm::standard(21);
    form.username = CommonField { label: "Access key ID", required: true };
    form.options = vec![
        OptionField { key: "bucket", label: "Bucket", required: true, kind: OptionKind::Text { default: "" } },
        OptionField { key: "region", label: "Region", required: false, kind: OptionKind::Text { default: "" } },
        OptionField { key: "token", label: "Session token", required: false, kind: OptionKind::Secret },
        OptionField {
            key: "security",
            label: "Security",
            required: false,
            kind: OptionKind::Choice { choices: SECURITY, default: "none" },
        },
        OptionField {
            key: "path_style",
            label: "Path-style",
            required: false,
            kind: OptionKind::Toggle { default: false },
        },
    ];
    form
}

fn draft(port: &str) -> ProfileDraft {
    ProfileDraft {
        protocol: "ftp".to_string(),
        name: " prod ".to_string(),
        host: " server.example.com ".to_string(),
        port: port.to_string(),
        username: " deploy ".to_string(),
        password: String::new(),
        remote_path: String::new(),
        options: BTreeMap::from([
            ("bucket".to_string(), " media ".to_string()),
            ("security".to_string(), "explicit".to_string()),
            ("path_style".to_string(), "true".to_string()),
        ]),
    }
}

fn entry(protocol: &str, options: &[(&str, &str)]) -> ConnectionEntry {
    ConnectionEntry {
        name: "prod".to_string(),
        protocol: protocol.to_string(),
        host: "old".to_string(),
        port: 21,
        username: "u".to_string(),
        password: None,
        options: options.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect(),
        source: ConnectionSource::Profile,
    }
}

#[test]
fn a_valid_draft_is_trimmed_into_a_profile_of_its_protocol() {
    let profile = draft("2222").validate(&form(), None).unwrap();

    assert_eq!(
        (profile.name.as_str(), profile.host.as_str(), profile.port, profile.username.as_str()),
        ("prod", "server.example.com", Some(2222), "deploy")
    );
    assert_eq!(profile.protocol, "ftp");
    assert_eq!(profile.password, None);
    assert_eq!(
        profile.options,
        BTreeMap::from([
            ("bucket".to_string(), "media".to_string()),
            ("path_style".to_string(), "true".to_string()),
            ("security".to_string(), "explicit".to_string()),
        ])
    );
}

#[test]
fn an_empty_port_takes_the_forms_default() {
    assert_eq!(draft(" ").validate(&form(), None).unwrap().port, Some(21));
}

#[test]
fn a_non_numeric_or_zero_port_is_rejected_by_its_label() {
    assert_eq!(draft("abc").validate(&form(), None).unwrap_err(), "Port must be a number from 1-65535");
    assert_eq!(draft("0").validate(&form(), None).unwrap_err(), "Port must be a number from 1-65535");
}

#[test]
fn empty_required_common_fields_are_rejected_by_their_label() {
    let mut empty = draft("21");
    empty.name = " ".to_string();
    assert_eq!(empty.validate(&form(), None).unwrap_err(), "Name can't be empty");
    empty.name = "n".to_string();
    empty.host = String::new();
    assert_eq!(empty.validate(&form(), None).unwrap_err(), "Host can't be empty");
    empty.host = "h".to_string();
    empty.username = String::new();
    assert_eq!(empty.validate(&form(), None).unwrap_err(), "Access key ID can't be empty");
}

#[test]
fn a_required_password_must_be_given() {
    let mut form = form();
    form.password.required = true;

    assert_eq!(draft("21").validate(&form, None).unwrap_err(), "Password can't be empty");
}

#[test]
fn a_required_option_must_be_given() {
    let mut missing = draft("21");
    missing.options.insert("bucket".to_string(), "  ".to_string());

    assert_eq!(missing.validate(&form(), None).unwrap_err(), "Bucket can't be empty");
}

#[test]
fn a_choice_outside_the_list_is_rejected_with_the_choice_labels() {
    let mut odd = draft("21");
    odd.options.insert("security".to_string(), "implicit".to_string());

    assert_eq!(odd.validate(&form(), None).unwrap_err(), "Security must be one of: None, Explicit TLS");
}

#[test]
fn a_toggle_must_be_true_or_false() {
    let mut odd = draft("21");
    odd.options.insert("path_style".to_string(), "yes".to_string());

    assert_eq!(odd.validate(&form(), None).unwrap_err(), "Path-style must be on or off");
}

#[test]
fn empty_optional_options_are_not_saved() {
    let mut sparse = draft("21");
    sparse.options.insert("region".to_string(), "   ".to_string());

    assert!(!sparse.validate(&form(), None).unwrap().options.contains_key("region"));
}

#[test]
fn passwords_and_secret_options_are_kept_verbatim() {
    let mut spaced = draft("21");
    spaced.password = " pw ".to_string();
    spaced.options.insert("token".to_string(), " tok ".to_string());

    let profile = spaced.validate(&form(), None).unwrap();

    assert_eq!(profile.password.as_deref(), Some(" pw "));
    assert_eq!(profile.options.get("token").map(String::as_str), Some(" tok "));
}

#[test]
fn the_remote_folder_is_saved_trimmed_and_dropped_when_empty() {
    let mut with_folder = draft("21");
    with_folder.remote_path = " /var/www ".to_string();
    let saved = with_folder.validate(&form(), None).unwrap();
    assert_eq!(saved.options.get("remote_path").map(String::as_str), Some("/var/www"));

    with_folder.remote_path = "  ".to_string();
    assert!(!with_folder.validate(&form(), None).unwrap().options.contains_key("remote_path"));
}

#[test]
fn editing_with_the_same_protocol_keeps_options_the_form_does_not_describe() {
    let original = entry("ftp", &[("identity_file", "/k"), ("foo", "bar"), ("bucket", "old")]);

    let profile = draft("21").validate(&form(), Some(&original)).unwrap();

    assert_eq!(profile.options.get("foo").map(String::as_str), Some("bar"));
    assert_eq!(profile.options.get("identity_file").map(String::as_str), Some("/k"));
    assert_eq!(profile.options.get("bucket").map(String::as_str), Some("media"));
}

#[test]
fn a_cleared_remote_folder_is_not_brought_back_from_the_saved_profile() {
    let original = entry("ftp", &[("remote_path", "/old")]);

    assert!(!draft("21").validate(&form(), Some(&original)).unwrap().options.contains_key("remote_path"));
}

#[test]
fn switching_protocol_drops_the_old_protocols_options() {
    let original = entry("sftp", &[("identity_file", "/k"), ("foo", "bar")]);

    let profile = draft("21").validate(&form(), Some(&original)).unwrap();

    assert!(!profile.options.contains_key("identity_file"));
    assert!(!profile.options.contains_key("foo"));
}

#[test]
fn debug_never_prints_option_values_or_the_password() {
    let mut secret = draft("21");
    secret.password = "hunter2".to_string();
    secret.options.insert("token".to_string(), "s3cr3t".to_string());

    let printed = format!("{secret:?}");

    assert!(!printed.contains("hunter2"));
    assert!(!printed.contains("s3cr3t"));
    assert!(printed.contains("token"));
}

#[test]
fn a_required_secret_of_only_spaces_counts_as_empty() {
    let mut form = form();
    form.options.push(OptionField { key: "key", label: "Secret key", required: true, kind: OptionKind::Secret });
    let mut blank = draft("21");
    blank.options.insert("key".to_string(), "   ".to_string());

    assert_eq!(blank.validate(&form, None).unwrap_err(), "Secret key can't be empty");
}
