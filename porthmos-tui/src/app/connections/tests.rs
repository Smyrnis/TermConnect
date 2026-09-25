use std::{collections::BTreeMap, path::Path};

use crossterm::event::KeyEventState;
use porthmos_core::{Answer, profiles::ProfileDraft};

use super::*;
use porthmos_core::{ConnectionForm, OptionField, OptionKind, ProtocolInfo};

use crate::{
    app::testing::{TestApp, test_app, test_app_with_protocols},
    widgets::dialog::confirm::ConfirmFocus,
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
    test.app.connections = entries;
    test.app.connections_cursor = 0;
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
    test.app.connections_cursor = 5;

    test.app.apply_core_event(Event::Profiles(vec![connection("a", ConnectionSource::Profile)]));

    assert_eq!(test.app.connections.len(), 1);
    assert_eq!(test.app.connections_cursor, 0);
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
        Some(Dialog::TextInput(ref dialog)) => {
            assert_eq!(dialog.title, "Password for u@srv");
            assert!(dialog.masked);
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

    assert_eq!(test.sent(), vec![Command::Answer { request_id: 4, answer: Some(Answer::Password("pw".to_string())) }]);
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
    assert_eq!(test.sent(), vec![Command::Answer { request_id: 4, answer: None }]);
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
    assert_eq!(test.sent(), vec![Command::Answer { request_id: 9, answer: Some(Answer::Confirmed) }]);
    assert_eq!(test.app.connection_status, ConnectionStatus::Connecting("web".to_string()));
}

#[test]
fn declining_the_host_key_cancels_the_connection() {
    let mut test = app();
    ask_to_trust_a_host_key(&mut test);

    test.app.apply_dialog_key(key(KeyCode::Enter));

    assert!(test.app.dialog.is_none());
    assert_eq!(test.sent(), vec![Command::Answer { request_id: 9, answer: None }]);
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
            draft: ProfileDraft {
                name: "prod".to_string(),
                host: "server.example.com".to_string(),
                port: "2222".to_string(),
                username: "deploy".to_string(),
                password: "hunter2".to_string(),
                protocol: "sftp".to_string(),
                remote_path: String::new(),
                options: BTreeMap::from([("identity_file".to_string(), String::new())]),
            },
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
