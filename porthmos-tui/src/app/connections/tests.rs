use std::{collections::BTreeMap, path::Path};

use crossterm::event::KeyEventState;
use porthmos_core::{Answer, ConnectionForm, OptionField, OptionKind, ProtocolInfo, profiles::ProfileDraft};

use super::*;
use crate::{
    app::testing::{TestApp, test_app, test_app_with_protocols},
    widgets::{dialog::confirm::ConfirmFocus, filter_line::FilterLine},
};

fn app() -> TestApp {
    test_app(Path::new("/d"))
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}

fn connection(name: &str, source: ConnectionSource) -> ConnectionEntry {
    ConnectionEntry {
        name: name.to_string(),
        protocol: "sftp".to_string(),
        host: format!("{name}.example.com"),
        port: 22,
        username: "deploy".to_string(),
        password: None,
        options: Default::default(),
        source,
        group: None,
        tags: Vec::new(),
        saved_password: false,
        in_keyring: Vec::new(),
    }
}

fn sample_protocols() -> Vec<ProtocolInfo> {
    let mut sftp = ConnectionForm::standard(22);
    sftp.options.push(OptionField {
        key: "identity_file",
        label: "Identity file",
        required: false,
        kind: OptionKind::Text { default: "" },
    });
    vec![
        ProtocolInfo { id: "sftp", display_name: "SFTP", form: sftp },
        ProtocolInfo { id: "ftp", display_name: "FTP", form: ConnectionForm::standard(21) },
    ]
}

fn screen_with(protocols: Vec<ProtocolInfo>, entries: Vec<ConnectionEntry>) -> TestApp {
    let mut test = test_app_with_protocols(Path::new("/d"), protocols);
    test.app.screen = Screen::Connections;
    test.app.connections.replace(entries);
    test.app.connections.cursor = 0;
    test
}

fn on_connections_screen(entries: Vec<ConnectionEntry>) -> TestApp {
    screen_with(sample_protocols(), entries)
}

fn set_field(test: &mut TestApp, key: &str, value: &str) {
    if let Some(Dialog::Form(form)) = test.app.dialog.as_mut()
        && let Some(field) = form.fields.iter_mut().find(|field| field.key == key)
    {
        field.value = value.to_string();
    }
}

fn fill_form(test: &mut TestApp, values: [&str; 4]) {
    for (key, value) in ["name", "host", "port", "username"].into_iter().zip(values) {
        set_field(test, key, value);
    }
}

#[test]
fn open_connections_switches_screen_and_loads_entries() {
    let mut test = app();

    test.app.apply_action(Action::OpenConnections);

    assert_eq!(test.app.screen, Screen::Connections);
    assert_eq!(test.sent(), vec![Command::ListProfiles]);
}

#[test]
fn the_profile_list_arrives_from_the_core() {
    let mut test = app();
    test.app.connections.cursor = 5;

    test.app.apply_core_event(Event::Profiles(vec![connection("a", ConnectionSource::Profile)]));

    assert_eq!(test.app.connections.entries().len(), 1);
    assert_eq!(test.app.connections.cursor, 0);
}

#[test]
fn connecting_asks_the_core_and_shows_the_attempt() {
    let mut test = on_connections_screen(vec![connection("prod", ConnectionSource::Profile)]);

    test.app.apply_action(Action::Open);
    test.app.apply_core_event(Event::Connecting { name: "prod".to_string() });

    assert_eq!(test.sent(), vec![Command::Connect { profile: "prod".to_string() }]);
    assert_eq!(test.app.connection_status, ConnectionStatus::Connecting("prod".to_string()));
}

#[test]
fn connecting_to_an_unreachable_host_reports_failure() {
    let mut test = app();
    test.app.connection_status = ConnectionStatus::Connecting("unreachable".to_string());

    test.app.apply_core_event(Event::ConnectFailed {
        name: "unreachable".to_string(),
        message: "Unable to connect to unreachable:\nConnection refused".to_string(),
    });

    match test.app.connection_status {
        ConnectionStatus::Failed(_) => {}
        ref other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(test.app.notifications.current().unwrap().severity, Severity::Error);
}

#[test]
fn a_new_session_gets_a_tab_and_becomes_active() {
    let mut test = app();

    test.app.apply_core_event(Event::Connected { session: 7, name: "prod".to_string(), shell_available: true });

    assert_eq!(test.app.sessions.active().unwrap().id, 7);
    assert_eq!(test.app.connection_status, ConnectionStatus::Disconnected);
}

#[test]
fn panel_event_listed_updates_the_matching_sessions_panel() {
    let mut test = app();
    let id = test.connect(1, "test");

    test.list_remote(id, "/home/user", Vec::new());

    assert_eq!(test.app.sessions.active().unwrap().panel.path(), Path::new("/home/user"));
}

#[test]
fn panel_event_listed_for_a_vanished_session_is_dropped() {
    let mut test = app();

    test.list_remote(999, "/x", Vec::new());

    assert!(test.app.sessions.is_empty());
}

#[test]
fn panel_event_failed_shows_a_notification_when_the_session_still_exists() {
    let mut test = app();
    test.connect(1, "test");

    test.app.apply_core_event(Event::Notice { severity: Severity::Error, message: "boom".to_string() });

    assert_eq!(test.notification().as_deref(), Some("boom"));
}

#[test]
fn connecting_to_an_already_connected_host_switches_instead_of_reconnecting() {
    let mut test = on_connections_screen(vec![connection("test", ConnectionSource::Profile)]);
    let first = test.connect(1, "test");
    test.connect(2, "other");

    test.app.connect_to_selected();

    assert_eq!(test.app.sessions.active_id(), Some(first));
    assert!(test.sent().is_empty());
    assert_eq!(test.app.connection_status, ConnectionStatus::Disconnected);
}

#[test]
fn disconnect_selected_reports_a_confirmation_notification() {
    let mut test = on_connections_screen(vec![connection("test", ConnectionSource::Profile)]);
    let session = test.connect(1, "test");

    test.app.apply_action(Action::Delete);

    assert_eq!(test.sent(), vec![Command::Disconnect { session }]);
    test.app
        .apply_core_event(Event::Notice { severity: Severity::Info, message: "Disconnected from test".to_string() });
    test.app.apply_core_event(Event::Disconnected { session, name: "test".to_string() });
    assert!(test.app.sessions.is_empty());
    assert_eq!(test.notification().as_deref(), Some("Disconnected from test"));
}

#[test]
fn a_password_question_opens_a_masked_prompt() {
    let mut test = app();

    test.app.apply_core_event(Event::Question {
        request_id: 4,
        question: Question::Password { username: "u".to_string(), name: "srv".to_string() },
    });

    match test.app.dialog {
        Some(Dialog::Form(ref dialog)) => {
            assert_eq!(dialog.title, "Password for u@srv");
            assert_eq!(dialog.fields[0].key, "password");
            assert_eq!(dialog.fields[0].kind, crate::widgets::dialog::FieldKind::Masked);
        }
        _ => panic!("expected the password prompt"),
    }
}

#[test]
fn submitting_the_password_answers_the_question() {
    let mut test = app();
    test.app.apply_core_event(Event::Question {
        request_id: 4,
        question: Question::Password { username: "u".to_string(), name: "srv".to_string() },
    });

    for c in "pw".chars() {
        test.app.apply_dialog_key(key(KeyCode::Char(c)));
    }
    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::Answer { request_id: 4, answer: Some(Answer::Password("pw".to_string())), save: false }]
    );
}

#[test]
fn escaping_the_password_prompt_cancels_the_connection() {
    let mut test = app();
    test.app.connection_status = ConnectionStatus::Connecting("srv".to_string());
    test.app.apply_core_event(Event::Question {
        request_id: 4,
        question: Question::Password { username: "u".to_string(), name: "srv".to_string() },
    });

    test.app.apply_dialog_key(key(KeyCode::Esc));

    assert!(test.app.dialog.is_none());
    assert_eq!(test.sent(), vec![Command::Answer { request_id: 4, answer: None, save: false }]);
    assert_eq!(test.app.connection_status, ConnectionStatus::Disconnected);
}

fn ask_to_trust_a_host_key(test: &mut TestApp) {
    test.app.connection_status = ConnectionStatus::Connecting("web".to_string());
    test.app.apply_core_event(Event::Question {
        request_id: 9,
        question: Question::TrustHostKey {
            name: "web".to_string(),
            host: "example.com".to_string(),
            port: 2222,
            key_type: "ssh-ed25519".to_string(),
            fingerprint: "SHA256:abc".to_string(),
        },
    });
}

#[test]
fn a_host_key_question_shows_the_host_and_fingerprint_with_no_focused() {
    let mut test = app();

    ask_to_trust_a_host_key(&mut test);

    match test.app.dialog {
        Some(Dialog::Confirm(ref dialog)) => {
            assert_eq!(
                dialog.message,
                "web (example.com:2222) is not a known host.\n\
                 ssh-ed25519 SHA256:abc\n\
                 Trust this key and add it to ~/.ssh/known_hosts?"
            );
            assert_eq!(dialog.focus, ConfirmFocus::No);
        }
        _ => panic!("expected the host key confirmation"),
    }
}

#[test]
fn confirming_the_host_key_trusts_it() {
    let mut test = app();
    ask_to_trust_a_host_key(&mut test);

    test.app.apply_dialog_key(key(KeyCode::Char('y')));

    assert!(test.app.dialog.is_none());
    assert_eq!(test.sent(), vec![Command::Answer { request_id: 9, answer: Some(Answer::Confirmed), save: false }]);
    assert_eq!(test.app.connection_status, ConnectionStatus::Connecting("web".to_string()));
}

#[test]
fn declining_the_host_key_cancels_the_connection() {
    let mut test = app();
    ask_to_trust_a_host_key(&mut test);

    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert!(test.app.dialog.is_none());
    assert_eq!(test.sent(), vec![Command::Answer { request_id: 9, answer: None, save: false }]);
    assert_eq!(test.app.connection_status, ConnectionStatus::Disconnected);
}

#[test]
fn add_connection_dialog_saves_a_new_profile_on_submit() {
    let mut test = on_connections_screen(Vec::new());

    test.app.apply_action(Action::AddConnection);
    assert!(matches!(test.app.dialog, Some(Dialog::Form(_))));
    fill_form(&mut test, ["prod", "server.example.com", "2222", "deploy"]);
    set_field(&mut test, "password", "hunter2");
    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::SaveProfile {
            original: None,
            draft: Box::new(ProfileDraft {
                name: "prod".to_string(),
                host: "server.example.com".to_string(),
                port: "2222".to_string(),
                username: "deploy".to_string(),
                password: porthmos_core::profiles::SecretEdit::Replace("hunter2".into()),
                protocol: "sftp".to_string(),
                remote_path: String::new(),
                options: BTreeMap::from([("identity_file".to_string(), String::new())]),
                group: String::new(),
                tags: String::new(),
                secret_options: Default::default(),
            }),
        }]
    );
    test.app.apply_core_event(Event::ProfileSaved);
    assert!(test.app.dialog.is_none());
}

#[test]
fn add_connection_dialog_keeps_the_dialog_open_on_invalid_port() {
    let mut test = on_connections_screen(Vec::new());
    test.app.apply_action(Action::AddConnection);
    fill_form(&mut test, ["prod", "server.example.com", "not-a-port", "deploy"]);
    test.app.apply_dialog_key(key(KeyCode::Enter));

    test.app.apply_core_event(Event::ProfileRejected { message: "Port must be a number from 1-65535".to_string() });

    match test.app.dialog {
        Some(Dialog::Form(ref form)) => assert!(form.error.is_some()),
        _ => panic!("expected the form dialog to stay open with an error"),
    }
}

#[test]
fn add_connection_dialog_keeps_the_dialog_open_on_port_zero() {
    let mut test = on_connections_screen(Vec::new());
    test.app.apply_action(Action::AddConnection);
    fill_form(&mut test, ["prod", "server.example.com", "0", "deploy"]);
    test.app.apply_dialog_key(key(KeyCode::Enter));

    test.app.apply_core_event(Event::ProfileRejected { message: "Port must be a number from 1-65535".to_string() });

    match test.app.dialog {
        Some(Dialog::Form(ref form)) => assert_eq!(form.error.as_deref(), Some("Port must be a number from 1-65535")),
        _ => panic!("expected the form dialog to stay open with an error"),
    }
}

#[test]
fn add_connection_dialog_rejects_a_name_that_already_exists() {
    let mut test = on_connections_screen(Vec::new());
    test.app.apply_action(Action::AddConnection);
    fill_form(&mut test, ["prod", "new.example.com", "22", "deploy"]);
    test.app.apply_dialog_key(key(KeyCode::Enter));

    test.app
        .apply_core_event(Event::ProfileRejected { message: "A connection named \"prod\" already exists".to_string() });

    match test.app.dialog {
        Some(Dialog::Form(ref form)) => assert!(form.error.is_some()),
        _ => panic!("expected the form dialog to stay open with an error"),
    }
}

#[test]
fn edit_connection_rejects_renaming_onto_an_existing_name() {
    let mut test = on_connections_screen(vec![connection("prod", ConnectionSource::Profile)]);
    test.app.apply_action(Action::Rename);
    set_field(&mut test, "name", "staging");
    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert!(matches!(
        test.sent().as_slice(),
        [Command::SaveProfile { original: Some(original), draft }] if original == "prod" && draft.name == "staging"
    ));
    test.app.apply_core_event(Event::ProfileRejected {
        message: "A connection named \"staging\" already exists".to_string(),
    });
    assert!(matches!(test.app.dialog, Some(Dialog::Form(ref form)) if form.error.is_some()));
}

#[test]
fn add_connection_action_does_nothing_outside_the_connections_screen() {
    let mut test = app();
    test.app.apply_action(Action::AddConnection);
    assert!(test.app.dialog.is_none());
}

#[test]
fn add_connection_dialog_stays_open_when_saving_fails() {
    let mut test = on_connections_screen(Vec::new());
    test.app.apply_action(Action::AddConnection);
    fill_form(&mut test, ["prod", "server.example.com", "22", "deploy"]);
    test.app.apply_dialog_key(key(KeyCode::Enter));

    test.app.apply_core_event(Event::Notice { severity: Severity::Error, message: "Not a directory".to_string() });

    assert!(test.app.dialog.is_some(), "the form must stay open so the user's input isn't lost on a save error");
    assert!(test.app.notifications.current().is_some());
}

#[test]
fn editing_a_connection_sends_its_identity_file_and_remote_folder_from_the_form() {
    let mut entry = connection("prod", ConnectionSource::Profile);
    entry.options = BTreeMap::from([
        ("identity_file".to_string(), "/home/user/.ssh/id_ed25519".to_string()),
        ("remote_path".to_string(), "/var/www".to_string()),
    ]);
    let mut test = on_connections_screen(vec![entry]);

    test.app.apply_action(Action::Rename);
    set_field(&mut test, "host", "new.example.com");
    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert!(matches!(
        test.sent().as_slice(),
        [Command::SaveProfile { original: Some(original), draft }]
            if original == "prod"
                && draft.host == "new.example.com"
                && draft.remote_path == "/var/www"
                && draft.options.get("identity_file").map(String::as_str) == Some("/home/user/.ssh/id_ed25519")
    ));
}

#[test]
fn adding_a_connection_without_any_protocol_shows_a_notice_instead_of_a_form() {
    let mut test = screen_with(Vec::new(), Vec::new());

    test.app.apply_action(Action::AddConnection);

    assert!(test.app.dialog.is_none());
    assert_eq!(
        test.app.notifications.current().map(|notification| notification.message.as_str()),
        Some("No protocols are available in this build")
    );
}

#[test]
fn changing_the_protocol_choice_rebuilds_the_open_form() {
    let mut test = on_connections_screen(Vec::new());
    test.app.apply_action(Action::AddConnection);

    test.app.apply_dialog_key(key(KeyCode::Right));

    let Some(Dialog::Form(form)) = &test.app.dialog else { panic!("the form closed") };
    assert_eq!(form.value("protocol").as_deref(), Some("ftp"));
    assert_eq!(form.value("port").as_deref(), Some("21"));
    assert!(form.value("identity_file").is_none());
}

#[test]
fn renaming_a_connection_to_a_new_name_deletes_the_old_profile() {
    let mut test = on_connections_screen(vec![connection("prod", ConnectionSource::Profile)]);

    test.app.apply_action(Action::Rename);
    set_field(&mut test, "name", "production");
    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert!(matches!(
        test.sent().as_slice(),
        [Command::SaveProfile { original: Some(original), draft }] if original == "prod" && draft.name == "production"
    ));
}

#[test]
fn delete_connection_action_opens_a_confirm_dialog() {
    let mut test = on_connections_screen(vec![connection("prod", ConnectionSource::Profile)]);

    test.app.apply_action(Action::DeleteConnection);

    assert!(matches!(test.app.dialog, Some(Dialog::Confirm(_))));
}

#[test]
fn confirming_delete_connection_removes_the_saved_profile() {
    let mut test = on_connections_screen(vec![connection("prod", ConnectionSource::Profile)]);

    test.app.apply_action(Action::DeleteConnection);
    test.app.apply_dialog_key(key(KeyCode::Char('y')));

    assert!(test.app.dialog.is_none());
    assert_eq!(test.sent(), vec![Command::DeleteProfile { name: "prod".to_string() }]);
}

#[test]
fn delete_connection_on_an_ssh_config_entry_does_not_open_a_dialog() {
    let mut test = on_connections_screen(vec![connection("prod", ConnectionSource::SshConfig)]);

    test.app.apply_action(Action::DeleteConnection);

    assert!(test.app.dialog.is_none());
    assert!(test.app.notifications.current().is_some());
}

#[test]
fn editing_a_profile_with_an_unknown_saved_choice_opens_on_the_default() {
    let mut protocols = sample_protocols();
    protocols[1].form.options.push(OptionField {
        key: "security",
        label: "Security",
        required: false,
        kind: OptionKind::Choice { choices: SECURITY_CHOICES, default: "explicit" },
    });
    let mut entry = connection("files", ConnectionSource::Profile);
    entry.protocol = "ftp".to_string();
    entry.options = BTreeMap::from([("security".to_string(), "weird".to_string())]);
    let mut test = screen_with(protocols, vec![entry]);

    test.app.apply_action(Action::Rename);

    let Some(Dialog::Form(form)) = &test.app.dialog else { panic!("the edit form did not open") };
    assert_eq!(form.value("security").as_deref(), Some("explicit"));
}

const SECURITY_CHOICES: &[porthmos_core::Choice] = &[
    porthmos_core::Choice { value: "none", label: "None" },
    porthmos_core::Choice { value: "explicit", label: "Explicit TLS" },
];

fn ask_to_trust_a_certificate(test: &mut TestApp) {
    test.app.connection_status = ConnectionStatus::Connecting("nas".to_string());
    test.app.apply_core_event(Event::Question {
        request_id: 11,
        question: Question::TrustCertificate {
            name: "nas".to_string(),
            host: "192.168.1.10".to_string(),
            port: 21,
            fingerprint: "SHA256:abc".to_string(),
            subject: "CN=nas.local".to_string(),
            expires: "2027-03-01".to_string(),
        },
    });
}

#[test]
fn a_certificate_question_shows_subject_expiry_and_fingerprint_with_no_focused() {
    let mut test = app();
    ask_to_trust_a_certificate(&mut test);

    match test.app.dialog {
        Some(Dialog::Confirm(ref dialog)) => {
            assert_eq!(
                dialog.message,
                "nas (192.168.1.10:21) presented a certificate that isn't trusted.\n\
                 Subject: CN=nas.local   Expires: 2027-03-01\n\
                 SHA256:abc\n\
                 Trust it and remember it?"
            );
            assert_eq!(dialog.focus, ConfirmFocus::No);
        }
        _ => panic!("expected the certificate confirmation"),
    }
}

#[test]
fn confirming_the_certificate_trusts_it() {
    let mut test = app();
    ask_to_trust_a_certificate(&mut test);

    test.app.apply_dialog_key(key(KeyCode::Char('y')));

    assert_eq!(test.sent(), vec![Command::Answer { request_id: 11, answer: Some(Answer::Confirmed), save: false }]);
}

#[test]
fn declining_the_certificate_cancels_the_connection() {
    let mut test = app();
    ask_to_trust_a_certificate(&mut test);

    test.app.apply_dialog_key(key(KeyCode::Esc));

    assert_eq!(test.sent(), vec![Command::Answer { request_id: 11, answer: None, save: false }]);
    assert_eq!(test.app.connection_status, ConnectionStatus::Disconnected);
}

fn grouped(name: &str, group: &str) -> ConnectionEntry {
    let mut entry = connection(name, ConnectionSource::Profile);
    entry.group = Some(group.to_string());
    entry
}

#[test]
fn enter_on_a_group_toggles_it_and_enter_on_a_connection_connects() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app.connections.replace(vec![grouped("web", "Work")]);

    test.app.apply_key(key(KeyCode::Enter));
    assert_eq!(test.app.connections.rows().len(), 2);
    assert!(test.sent().is_empty());

    test.app.apply_key(key(KeyCode::Down));
    test.app.apply_key(key(KeyCode::Enter));
    assert_eq!(test.sent(), vec![Command::Connect { profile: "web".into() }]);
}

#[test]
fn right_opens_and_left_closes_a_group_on_the_connections_screen() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app.connections.replace(vec![grouped("web", "Work")]);

    test.app.apply_key(key(KeyCode::Right));
    assert_eq!(test.app.connections.rows().len(), 2);
    test.app.apply_key(key(KeyCode::Left));
    assert_eq!(test.app.connections.rows().len(), 1);
}

#[test]
fn slash_filters_the_connections_and_esc_clears_before_leaving() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app
        .connections
        .replace(vec![connection("web", ConnectionSource::Profile), connection("db", ConnectionSource::Profile)]);

    test.app.apply_key(key(KeyCode::Char('/')));
    test.app.apply_key(key(KeyCode::Char('w')));
    test.app.apply_key(key(KeyCode::Enter));
    assert_eq!(test.app.connections.rows().len(), 1);

    test.app.apply_key(key(KeyCode::Esc));
    assert_eq!(test.app.connections.rows().len(), 2);
    assert_eq!(test.app.screen, Screen::Connections);
    test.app.apply_key(key(KeyCode::Esc));
    assert_eq!(test.app.screen, Screen::Files);
}

#[test]
fn typing_in_the_filter_narrows_and_another_key_closes_the_line_and_acts() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app
        .connections
        .replace(vec![connection("web", ConnectionSource::Profile), connection("db", ConnectionSource::Profile)]);

    test.app.apply_key(key(KeyCode::Char('/')));
    test.app.apply_key(key(KeyCode::Char('d')));
    assert_eq!(test.app.connections.filter_status(40).as_deref(), Some("/d\u{2588} (1 of 2)"));
    test.app.apply_key(key(KeyCode::F(9)));

    assert!(!test.app.connections.editing_filter());
    assert_eq!(test.app.connections.filter(), Some("d"));
    assert_eq!(test.sent(), vec![Command::ListProfiles]);
}

#[test]
fn edit_and_delete_leave_orphan_label_rows_alone_and_enter_only_offers_the_fix() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app.connections.replace(vec![ConnectionEntry::orphan_labels(
        "web1".into(),
        Default::default(),
        ConnectionSource::ShadowedSshHost,
    )]);

    test.app.announced_missing.insert("web1".into());

    test.app.apply_action(Action::Rename);
    test.app.apply_action(Action::DeleteConnection);
    assert!(test.app.dialog.is_none());

    test.app.apply_action(Action::Open);
    assert!(matches!(test.app.dialog, Some(Dialog::List(_))));
    assert!(test.sent().is_empty());
}

#[test]
fn the_cursor_stays_on_the_same_connection_after_a_reload() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app
        .connections
        .replace(vec![connection("b", ConnectionSource::Profile), connection("c", ConnectionSource::Profile)]);
    test.app.apply_key(key(KeyCode::Down));

    test.app.apply_core_event(Event::Profiles(vec![
        connection("a", ConnectionSource::Profile),
        connection("b", ConnectionSource::Profile),
        connection("c", ConnectionSource::Profile),
    ]));

    assert_eq!(test.app.connections.selected_entry().map(|entry| entry.name.as_str()), Some("c"));
}

#[test]
fn f2_on_an_ssh_host_opens_the_labels_form_and_enter_sends_labels() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app.connections.replace(vec![connection("web1", ConnectionSource::SshConfig)]);

    test.app.apply_action(Action::Rename);
    let Some(Dialog::Form(form)) = &test.app.dialog else { panic!("no form") };
    assert_eq!(form.title, "Labels for web1");
    test.app.apply_key(key(KeyCode::Char('W')));
    test.app.apply_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::SaveSshLabels { name: "web1".into(), group: "W".into(), tags: String::new() }]
    );
}

#[test]
fn a_rejected_labels_save_shows_in_the_form() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app.connections.replace(vec![connection("web1", ConnectionSource::SshConfig)]);
    test.app.apply_action(Action::Rename);

    test.app.apply_core_event(Event::ProfileRejected { message: "web1 is not in ~/.ssh/config".into() });

    let Some(Dialog::Form(form)) = &test.app.dialog else { panic!("form closed") };
    assert_eq!(form.error.as_deref(), Some("web1 is not in ~/.ssh/config"));
}

#[test]
fn after_a_save_the_connection_is_revealed() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app.connections.replace(vec![connection("aaa", ConnectionSource::Profile)]);
    test.app.apply_action(Action::AddConnection);
    test.app.submit_connection_form(vec![("name", " web ".into()), ("group", "Work/Web".into())], None);

    test.app.apply_core_event(Event::ProfileSaved);
    let mut saved = connection("web", ConnectionSource::Profile);
    saved.group = Some("Work/Web".into());
    test.app.apply_core_event(Event::Profiles(vec![connection("aaa", ConnectionSource::Profile), saved]));

    assert_eq!(test.app.connections.selected_entry().map(|entry| entry.name.as_str()), Some("web"));
}

#[test]
fn after_saving_ssh_labels_the_host_is_revealed_in_its_group() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app
        .connections
        .replace(vec![connection("aaa", ConnectionSource::Profile), connection("web1", ConnectionSource::SshConfig)]);
    test.app.apply_key(key(KeyCode::Down));
    test.app.apply_action(Action::Rename);
    test.app.apply_key(key(KeyCode::Char('W')));
    test.app.apply_key(key(KeyCode::Enter));

    test.app.apply_core_event(Event::ProfileSaved);
    let mut labelled = connection("web1", ConnectionSource::SshConfig);
    labelled.group = Some("W".into());
    test.app.apply_core_event(Event::Profiles(vec![connection("aaa", ConnectionSource::Profile), labelled]));

    assert_eq!(test.app.connections.selected_entry().map(|entry| entry.name.as_str()), Some("web1"));
    assert!(test.app.dialog.is_none());
}

#[test]
fn renaming_and_regrouping_reveals_the_new_name() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    let mut old = connection("old", ConnectionSource::Profile);
    old.group = Some("A".into());
    test.app.connections.replace(vec![old]);
    test.app.submit_connection_form(vec![("name", "new".into()), ("group", "B/C".into())], Some("old".into()));

    test.app.apply_core_event(Event::ProfileSaved);
    let mut renamed = connection("new", ConnectionSource::Profile);
    renamed.group = Some("B/C".into());
    test.app.apply_core_event(Event::Profiles(vec![renamed]));

    assert_eq!(test.app.connections.selected_entry().map(|entry| entry.name.as_str()), Some("new"));
    assert_eq!(test.app.connections.rows().len(), 3);
}

#[test]
fn a_listing_without_a_save_does_not_move_the_cursor() {
    let mut test = app();
    test.app.screen = Screen::Connections;
    test.app
        .connections
        .replace(vec![connection("a", ConnectionSource::Profile), connection("b", ConnectionSource::Profile)]);
    test.app.submit_connection_form(vec![("name", "b".into())], None);
    test.app.apply_core_event(Event::ProfileRejected { message: "bad".into() });

    test.app.apply_core_event(Event::Profiles(vec![
        connection("a", ConnectionSource::Profile),
        connection("b", ConnectionSource::Profile),
    ]));

    assert_eq!(test.app.connections.cursor, 0);
}

#[test]
fn cancelling_the_form_forgets_the_pending_reveal() {
    let mut test = screen_with(
        sample_protocols(),
        vec![connection("a", ConnectionSource::Profile), connection("b", ConnectionSource::Profile)],
    );
    test.app.apply_action(Action::AddConnection);
    assert!(matches!(test.app.dialog, Some(Dialog::Form(_))));
    test.app.submit_connection_form(vec![("name", "b".into())], None);
    test.app.apply_key(key(KeyCode::Esc));

    test.app.apply_core_event(Event::ProfileSaved);
    test.app.apply_core_event(Event::Profiles(vec![
        connection("a", ConnectionSource::Profile),
        connection("b", ConnectionSource::Profile),
    ]));

    assert_eq!(test.app.connections.cursor, 0);
}

#[test]
fn delete_on_a_shadowed_labels_row_does_not_disconnect_the_same_named_profile() {
    let mut test = app();
    test.connect(5, "foo");
    test.app.screen = Screen::Connections;
    test.app.announced_missing.insert("foo".into());
    test.app.connections.replace(vec![
        connection("foo", ConnectionSource::Profile),
        ConnectionEntry::orphan_labels("foo".into(), Default::default(), ConnectionSource::ShadowedSshHost),
    ]);
    test.sent();
    test.app.connections.cursor = 1;

    test.app.apply_action(Action::Delete);

    assert!(test.sent().is_empty());
}

fn ask_password(test: &mut TestApp, request_id: u64) {
    test.app.apply_core_event(Event::Question {
        request_id,
        question: Question::Password { username: "u".into(), name: "web".into() },
    });
}

fn prompt(test: &TestApp) -> &crate::widgets::dialog::FormDialog {
    match &test.app.dialog {
        Some(Dialog::Form(form)) => form,
        _ => panic!("no password prompt"),
    }
}

fn type_text(test: &mut TestApp, text: &str) {
    for character in text.chars() {
        test.app.apply_key(key(KeyCode::Char(character)));
    }
}

#[test]
fn the_password_prompt_offers_saving_at_the_remembered_choice() {
    let mut test = app();
    test.app.apply_core_event(Event::SaveChoice { save: true });
    ask_password(&mut test, 7);

    assert_eq!(prompt(&test).title, "Password for u@web");
    assert_eq!(prompt(&test).fields[1].label, "Save in keyring");
    assert_eq!(prompt(&test).value("save").as_deref(), Some("true"));

    type_text(&mut test, "p w");
    test.app.apply_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::Answer { request_id: 7, answer: Some(Answer::Password("p w".into())), save: true }]
    );
    assert!(test.app.dialog.is_none());
}

#[test]
fn the_first_prompt_starts_at_no() {
    let mut test = app();
    ask_password(&mut test, 1);

    assert_eq!(prompt(&test).value("save").as_deref(), Some("false"));
}

#[test]
fn changing_the_choice_is_remembered_and_keeping_it_is_not() {
    let mut test = app();
    ask_password(&mut test, 1);
    test.app.apply_key(key(KeyCode::Tab));
    test.app.apply_key(key(KeyCode::Right));
    test.app.apply_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![
            Command::RememberSaveChoice { save: true },
            Command::Answer { request_id: 1, answer: Some(Answer::Password(String::new())), save: true },
        ]
    );

    ask_password(&mut test, 2);
    assert_eq!(prompt(&test).value("save").as_deref(), Some("true"));
    type_text(&mut test, "x");
    test.app.apply_key(key(KeyCode::Enter));
    assert_eq!(
        test.sent(),
        vec![Command::Answer { request_id: 2, answer: Some(Answer::Password("x".into())), save: true }]
    );
}

#[test]
fn without_a_keyring_the_prompt_has_no_save_field_and_never_asks_to_save() {
    let mut test = app();
    test.app.apply_core_event(Event::SaveChoice { save: true });
    test.app.apply_core_event(Event::KeyringStatus { available: false });
    ask_password(&mut test, 1);

    assert!(prompt(&test).value("save").is_none());
    assert_eq!(prompt(&test).fields.len(), 1);
    type_text(&mut test, "pw");
    test.app.apply_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::Answer { request_id: 1, answer: Some(Answer::Password("pw".into())), save: false }]
    );
}

#[test]
fn the_save_field_returns_when_the_keyring_becomes_available() {
    let mut test = app();
    test.app.apply_core_event(Event::KeyringStatus { available: false });
    test.app.apply_core_event(Event::KeyringStatus { available: true });
    ask_password(&mut test, 1);

    assert!(prompt(&test).value("save").is_some());
}

#[test]
fn escaping_the_new_prompt_still_cancels() {
    let mut test = app();
    ask_password(&mut test, 3);
    type_text(&mut test, "abc");

    test.app.apply_key(key(KeyCode::Esc));

    assert_eq!(test.sent(), vec![Command::Answer { request_id: 3, answer: None, save: false }]);
    assert!(test.app.dialog.is_none());
}

#[test]
fn sent_commands_never_print_the_typed_password() {
    let mut test = app();
    ask_password(&mut test, 1);
    type_text(&mut test, "hunter2");
    test.app.apply_key(key(KeyCode::Enter));

    assert!(!format!("{:?}", test.sent()).contains("hunter2"));
}

#[test]
fn without_a_keyring_the_connection_form_says_why() {
    let mut test = on_connections_screen(vec![connection("web", ConnectionSource::Profile)]);
    test.app.apply_action(Action::AddConnection);
    assert!(matches!(&test.app.dialog, Some(Dialog::Form(form)) if form.hint.is_none()));
    test.app.dialog = None;

    test.app.apply_core_event(Event::KeyringStatus { available: false });
    test.app.apply_action(Action::AddConnection);
    let hint = |test: &TestApp| match &test.app.dialog {
        Some(Dialog::Form(form)) => form.hint.clone(),
        _ => panic!("no form"),
    };
    assert_eq!(
        hint(&test).as_deref(),
        Some("Passwords are not saved: no system keyring. They are kept until Porthmos quits.")
    );
    test.app.dialog = None;

    test.app.apply_action(Action::Rename);
    assert!(hint(&test).is_some());
}

#[test]
fn the_labels_form_can_forget_a_saved_password() {
    let mut host = connection("web1", ConnectionSource::SshConfig);
    host.saved_password = true;
    let mut test = on_connections_screen(vec![host]);

    test.app.apply_action(Action::Rename);
    test.app.apply_key(key(KeyCode::Tab));
    test.app.apply_key(key(KeyCode::Tab));
    test.app.apply_key(key(KeyCode::Right));
    test.app.apply_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![
            Command::SaveSshLabels { name: "web1".into(), group: String::new(), tags: String::new() },
            Command::ForgetSshPassword { alias: "web1".into() },
        ]
    );
}

#[test]
fn keeping_the_saved_password_sends_no_forget() {
    let mut host = connection("web1", ConnectionSource::SshConfig);
    host.saved_password = true;
    let mut test = on_connections_screen(vec![host]);

    test.app.apply_action(Action::Rename);
    test.app.apply_key(key(KeyCode::Enter));

    assert_eq!(
        test.sent(),
        vec![Command::SaveSshLabels { name: "web1".into(), group: String::new(), tags: String::new() }]
    );
}

#[test]
fn the_status_line_shows_a_waiting_keyring() {
    use ratatui::{Terminal, backend::TestBackend};
    let status = |test: &TestApp| {
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal.draw(|frame| test.app.render_status(frame, frame.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>()
    };
    let mut test = app();

    test.app.apply_core_event(Event::KeyringWaiting { waiting: true });
    assert!(status(&test).contains("Waiting for the system keyring\u{2026}"), "{}", status(&test));

    test.app.apply_core_event(Event::KeyringWaiting { waiting: false });
    assert!(!status(&test).contains("Waiting for the system keyring"));
}

#[test]
fn the_password_prompt_shows_asterisks_never_the_password() {
    use ratatui::{Terminal, backend::TestBackend};
    let mut test = app();
    ask_password(&mut test, 1);
    type_text(&mut test, "s3cret");

    let mut terminal = Terminal::new(TestBackend::new(70, 10)).unwrap();
    terminal.draw(|frame| test.app.dialog.as_ref().unwrap().render(frame, frame.area())).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(screen.contains("******"), "{screen}");
    assert!(!screen.contains("s3cret"), "{screen}");
}
