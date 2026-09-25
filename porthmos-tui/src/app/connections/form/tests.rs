use std::collections::BTreeMap;

use porthmos_core::{
    Choice, ConnectionForm, OptionField, OptionKind, ProtocolInfo,
    profiles::{ConnectionEntry, ConnectionSource},
};

use super::*;
use crate::widgets::dialog::FieldKind;

const SECURITY: &[Choice] =
    &[Choice { value: "none", label: "None" }, Choice { value: "explicit", label: "Explicit TLS" }];

fn protocols() -> Vec<ProtocolInfo> {
    let mut ftp = ConnectionForm::standard(21);
    ftp.options = vec![
        OptionField {
            key: "security",
            label: "Security",
            required: false,
            kind: OptionKind::Choice { choices: SECURITY, default: "explicit" },
        },
        OptionField {
            key: "passive",
            label: "Passive mode",
            required: false,
            kind: OptionKind::Toggle { default: true },
        },
        OptionField { key: "token", label: "Token", required: false, kind: OptionKind::Secret },
    ];
    vec![
        ProtocolInfo { id: "sftp", display_name: "SFTP", form: ConnectionForm::standard(22) },
        ProtocolInfo { id: "ftp", display_name: "FTP", form: ftp },
    ]
}

fn keys(form: &FormDialog) -> Vec<&'static str> {
    form.fields.iter().map(|field| field.key).collect()
}

fn set(form: &mut FormDialog, key: &str, value: &str) {
    let field = form.fields.iter_mut().find(|field| field.key == key).unwrap();
    field.value = value.to_string();
}

fn select_protocol(form: &mut FormDialog, id: &str) {
    let field = form.fields.iter_mut().find(|field| field.key == "protocol").unwrap();
    if let FieldKind::Choice { choices, selected } = &mut field.kind {
        *selected = choices.iter().position(|(value, _)| value == id).unwrap();
    }
}

fn ftp_entry(options: &[(&str, &str)]) -> ConnectionEntry {
    ConnectionEntry {
        name: "files".to_string(),
        protocol: "ftp".to_string(),
        host: "ftp.example.com".to_string(),
        port: 2121,
        username: "u".to_string(),
        password: Some("pw".to_string()),
        options: options.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect(),
        source: ConnectionSource::Profile,
    }
}

#[test]
fn adding_starts_on_the_first_protocol_with_its_default_port() {
    let form = build("Add connection", &protocols(), None).unwrap();

    assert_eq!(keys(&form), ["protocol", "name", "host", "port", "username", "password", "remote_path"]);
    assert_eq!(form.value("protocol").as_deref(), Some("sftp"));
    assert_eq!(form.value("port").as_deref(), Some("22"));
    assert_eq!(form.focused, 0);
}

#[test]
fn adding_with_no_protocols_builds_no_form() {
    assert!(build("Add connection", &[], None).is_none());
}

#[test]
fn editing_fills_every_field_from_the_entry_including_options() {
    let entry = ftp_entry(&[("security", "none"), ("passive", "false"), ("remote_path", "/pub")]);

    let form = build("Edit connection", &protocols(), Some(&entry)).unwrap();

    assert_eq!(
        keys(&form),
        ["protocol", "name", "host", "port", "username", "password", "remote_path", "security", "passive", "token"]
    );
    assert_eq!(form.value("port").as_deref(), Some("2121"));
    assert_eq!(form.value("password").as_deref(), Some("pw"));
    assert_eq!(form.value("remote_path").as_deref(), Some("/pub"));
    assert_eq!(form.value("security").as_deref(), Some("none"));
    assert_eq!(form.value("passive").as_deref(), Some("false"));
}

#[test]
fn a_saved_choice_outside_the_list_opens_on_the_default() {
    let form = build("Edit connection", &protocols(), Some(&ftp_entry(&[("security", "weird")]))).unwrap();

    assert_eq!(form.value("security").as_deref(), Some("explicit"));
}

#[test]
fn option_kinds_map_to_field_kinds() {
    let form = build("Edit connection", &protocols(), Some(&ftp_entry(&[]))).unwrap();
    let kind = |key: &str| form.fields.iter().find(|field| field.key == key).unwrap().kind.clone();

    assert!(matches!(kind("security"), FieldKind::Choice { .. }));
    assert!(
        matches!(kind("passive"), FieldKind::Choice { ref choices, .. } if choices[0] == ("true".to_string(), "Yes".to_string()))
    );
    assert_eq!(kind("token"), FieldKind::Masked);
    assert_eq!(kind("password"), FieldKind::Masked);
}

#[test]
fn an_entry_of_an_unavailable_protocol_is_shown_as_not_available_with_the_standard_form() {
    let mut entry = ftp_entry(&[]);
    entry.protocol = "webdav".to_string();

    let form = build("Edit connection", &protocols(), Some(&entry)).unwrap();

    assert_eq!(form.value("protocol").as_deref(), Some("webdav"));
    assert!(matches!(
        &form.fields[0].kind,
        FieldKind::Choice { choices, selected } if choices[*selected].1 == "webdav (not available)"
    ));
    assert_eq!(keys(&form).len(), 7);
}

#[test]
fn switching_protocol_keeps_typed_values_moves_an_untouched_port_and_swaps_options() {
    let mut form = build("Add connection", &protocols(), None).unwrap();
    set(&mut form, "name", "files");
    set(&mut form, "host", "ftp.example.com");
    set(&mut form, "username", "u");
    set(&mut form, "password", "pw");
    set(&mut form, "remote_path", "/pub");
    form.error = Some("old error".to_string());

    select_protocol(&mut form, "ftp");
    rebuild_for_protocol(&mut form, &protocols());

    assert_eq!(form.value("name").as_deref(), Some("files"));
    assert_eq!(form.value("host").as_deref(), Some("ftp.example.com"));
    assert_eq!(form.value("password").as_deref(), Some("pw"));
    assert_eq!(form.value("remote_path").as_deref(), Some("/pub"));
    assert_eq!(form.value("port").as_deref(), Some("21"));
    assert_eq!(form.value("security").as_deref(), Some("explicit"));
    assert_eq!(form.focused, 0);
    assert_eq!(form.error, None);

    select_protocol(&mut form, "sftp");
    rebuild_for_protocol(&mut form, &protocols());

    assert_eq!(form.value("port").as_deref(), Some("22"));
    assert!(form.value("security").is_none());
    assert_eq!(form.value("name").as_deref(), Some("files"));
}

#[test]
fn switching_protocol_keeps_a_port_the_user_typed() {
    let mut form = build("Add connection", &protocols(), None).unwrap();
    set(&mut form, "port", "2200");

    select_protocol(&mut form, "ftp");
    rebuild_for_protocol(&mut form, &protocols());

    assert_eq!(form.value("port").as_deref(), Some("2200"));
}

#[test]
fn the_draft_carries_protocol_common_fields_remote_folder_and_options() {
    let entry = ftp_entry(&[("security", "none"), ("remote_path", "/pub")]);
    let form = build("Edit connection", &protocols(), Some(&entry)).unwrap();
    let values = form.fields.iter().map(|field| (field.key, field.submitted_value())).collect();

    let draft = draft(values);

    assert_eq!(draft.protocol, "ftp");
    assert_eq!((draft.name.as_str(), draft.port.as_str(), draft.password.as_str()), ("files", "2121", "pw"));
    assert_eq!(draft.remote_path, "/pub");
    assert_eq!(
        draft.options,
        BTreeMap::from([
            ("passive".to_string(), "true".to_string()),
            ("security".to_string(), "none".to_string()),
            ("token".to_string(), String::new()),
        ])
    );
}

fn sftp_with_identity() -> Vec<ProtocolInfo> {
    let mut protocols = protocols();
    protocols[0].form.options.push(OptionField {
        key: "identity_file",
        label: "Identity file",
        required: false,
        kind: OptionKind::Text { default: "" },
    });
    protocols
}

#[test]
fn switching_away_and_back_while_editing_restores_the_saved_options() {
    let mut entry = ftp_entry(&[("identity_file", "/home/u/.ssh/id")]);
    entry.protocol = "sftp".to_string();
    let mut form = build("Edit connection", &sftp_with_identity(), Some(&entry)).unwrap();

    select_protocol(&mut form, "ftp");
    rebuild_for_protocol(&mut form, &sftp_with_identity());
    select_protocol(&mut form, "sftp");
    rebuild_for_protocol(&mut form, &sftp_with_identity());

    assert_eq!(form.value("identity_file").as_deref(), Some("/home/u/.ssh/id"));
}

#[test]
fn switching_away_and_back_while_adding_restores_typed_options() {
    let mut form = build("Add connection", &protocols(), None).unwrap();
    select_protocol(&mut form, "ftp");
    rebuild_for_protocol(&mut form, &protocols());
    select_protocol_field(&mut form, "security", "none");
    set(&mut form, "token", "tok");

    select_protocol(&mut form, "sftp");
    rebuild_for_protocol(&mut form, &protocols());
    select_protocol(&mut form, "ftp");
    rebuild_for_protocol(&mut form, &protocols());

    assert_eq!(form.value("security").as_deref(), Some("none"));
    assert_eq!(form.value("token").as_deref(), Some("tok"));
}

fn select_protocol_field(form: &mut FormDialog, key: &str, value: &str) {
    let field = form.fields.iter_mut().find(|field| field.key == key).unwrap();
    if let FieldKind::Choice { choices, selected } = &mut field.kind {
        *selected = choices.iter().position(|(choice, _)| choice == value).unwrap();
    }
}

#[test]
fn a_typed_port_that_equals_another_protocols_default_is_kept_on_switch() {
    let mut form = build("Add connection", &protocols(), None).unwrap();
    set(&mut form, "port", "21");

    select_protocol(&mut form, "ftp");
    rebuild_for_protocol(&mut form, &protocols());
    select_protocol(&mut form, "sftp");
    rebuild_for_protocol(&mut form, &protocols());

    assert_eq!(form.value("port").as_deref(), Some("21"));
}

#[test]
fn editing_an_entry_on_its_protocols_default_port_follows_a_protocol_switch() {
    let mut entry = ftp_entry(&[]);
    entry.port = 21;

    let mut form = build("Edit connection", &protocols(), Some(&entry)).unwrap();
    select_protocol(&mut form, "sftp");
    rebuild_for_protocol(&mut form, &protocols());

    assert_eq!(form.value("port").as_deref(), Some("22"));
}

#[test]
fn editing_an_entry_on_a_custom_port_keeps_it_on_a_protocol_switch() {
    let mut form = build("Edit connection", &protocols(), Some(&ftp_entry(&[]))).unwrap();

    select_protocol(&mut form, "sftp");
    rebuild_for_protocol(&mut form, &protocols());

    assert_eq!(form.value("port").as_deref(), Some("2121"));
}
