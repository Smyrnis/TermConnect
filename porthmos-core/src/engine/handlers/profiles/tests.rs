use std::collections::BTreeMap;

use super::*;
use crate::{
    engine::{
        Command,
        testing::{TestEngine, test_engine},
    },
    profiles::ConnectionProfile,
};

fn engine_with_sftp() -> TestEngine {
    let mut t = test_engine();
    let mut form = porthmos_vfs::ConnectionForm::standard(22);
    form.options.push(porthmos_vfs::OptionField {
        key: "identity_file",
        label: "Identity file",
        required: false,
        kind: porthmos_vfs::OptionKind::Text { default: "" },
    });
    let protocol =
        porthmos_vfs::testing::FakeProtocol::new(porthmos_vfs::testing::FakeFs::new()).with_id("sftp").with_form(form);
    t.engine.protocols = vec![std::sync::Arc::new(protocol)];
    t
}

fn draft(name: &str, host: &str, port: &str) -> ProfileDraft {
    ProfileDraft {
        name: name.to_string(),
        host: host.to_string(),
        port: port.to_string(),
        username: "deploy".to_string(),
        protocol: "sftp".to_string(),
        ..Default::default()
    }
}

fn saved(t: &TestEngine) -> Vec<ConnectionProfile> {
    let mut profiles = store::load(&t.engine.paths).unwrap();
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    profiles
}

fn seed(t: &TestEngine, name: &str, host: &str, options: BTreeMap<String, String>) {
    store::save(
        &t.engine.paths,
        &ConnectionProfile {
            name: name.to_string(),
            protocol: "sftp".to_string(),
            host: host.to_string(),
            port: Some(22),
            username: "deploy".to_string(),
            password: None,
            options,
        },
    )
    .unwrap();
}

#[test]
fn saving_a_new_profile_stores_it_and_lists_the_profiles() {
    let mut t = engine_with_sftp();
    let mut new = draft("prod", "server.example.com", "2222");
    new.password = "hunter2".to_string();

    t.engine.handle_command(Command::SaveProfile { original: None, draft: new });

    let events = t.drain();
    assert!(matches!(events[0], Event::ProfileSaved));
    assert!(matches!(&events[1], Event::Profiles(entries) if entries.len() == 1));
    let saved = saved(&t);
    assert_eq!(
        (saved[0].name.as_str(), saved[0].host.as_str(), saved[0].port),
        ("prod", "server.example.com", Some(2222))
    );
    assert_eq!(saved[0].password.as_deref(), Some("hunter2"));
}

#[test]
fn an_invalid_draft_is_rejected_with_the_reason() {
    let mut t = engine_with_sftp();

    t.engine.handle_command(Command::SaveProfile { original: None, draft: draft("prod", "h", "not-a-port") });

    assert!(
        matches!(t.drain().as_slice(), [Event::ProfileRejected { message }] if message == "Port must be a number from 1-65535")
    );
    assert!(saved(&t).is_empty());
}

#[test]
fn add_connection_dialog_rejects_a_name_that_already_exists() {
    let mut t = engine_with_sftp();
    seed(&t, "prod", "original.example.com", BTreeMap::new());

    t.engine.handle_command(Command::SaveProfile { original: None, draft: draft("prod", "new.example.com", "22") });

    assert!(
        matches!(t.drain().as_slice(), [Event::ProfileRejected { message }] if message == "A connection named \"prod\" already exists")
    );
    assert_eq!(saved(&t)[0].host, "original.example.com");
}

#[test]
fn edit_connection_rejects_renaming_onto_an_existing_name() {
    let mut t = engine_with_sftp();
    seed(&t, "prod", "prod.example.com", BTreeMap::new());
    seed(&t, "staging", "staging.example.com", BTreeMap::new());

    t.engine.handle_command(Command::SaveProfile {
        original: Some("prod".to_string()),
        draft: draft("staging", "prod.example.com", "22"),
    });

    assert!(matches!(t.drain().as_slice(), [Event::ProfileRejected { .. }]));
    let saved = saved(&t);
    assert_eq!((saved[0].name.as_str(), saved[0].host.as_str()), ("prod", "prod.example.com"));
    assert_eq!((saved[1].name.as_str(), saved[1].host.as_str()), ("staging", "staging.example.com"));
}

#[test]
fn edit_connection_saves_the_identity_file_and_remote_path_sent_by_the_form() {
    let mut t = engine_with_sftp();
    let options = BTreeMap::from([
        ("identity_file".to_string(), "/home/user/.ssh/id_ed25519".to_string()),
        ("remote_path".to_string(), "/var/www".to_string()),
    ]);
    seed(&t, "prod", "old.example.com", options.clone());

    let mut edit = draft("prod", "new.example.com", "22");
    edit.remote_path = "/var/www".to_string();
    edit.options.insert("identity_file".to_string(), "/home/user/.ssh/id_ed25519".to_string());

    t.engine.handle_command(Command::SaveProfile { original: Some("prod".to_string()), draft: edit });

    let saved = saved(&t);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].host, "new.example.com");
    assert_eq!(saved[0].options, options);
}

#[test]
fn renaming_a_connection_to_a_new_name_deletes_the_old_profile() {
    let mut t = engine_with_sftp();
    seed(&t, "prod", "server.example.com", BTreeMap::new());

    t.engine.handle_command(Command::SaveProfile {
        original: Some("prod".to_string()),
        draft: draft("production", "server.example.com", "22"),
    });

    let saved = saved(&t);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "production");
}

#[test]
fn a_save_that_fails_reports_the_error_without_closing_the_form() {
    let mut t = engine_with_sftp();
    std::fs::write(&t.engine.paths.config_dir, b"not a directory").ok();
    std::fs::create_dir_all(t.engine.paths.config_dir.parent().unwrap()).unwrap();
    std::fs::write(&t.engine.paths.config_dir, b"not a directory").unwrap();

    t.engine.handle_command(Command::SaveProfile { original: None, draft: draft("prod", "h", "22") });

    let events = t.drain();
    assert!(matches!(events.as_slice(), [Event::Notice { severity: Severity::Error, .. }]), "{events:?}");
}

#[test]
fn confirming_delete_connection_removes_the_saved_profile() {
    let mut t = engine_with_sftp();
    seed(&t, "prod", "server.example.com", BTreeMap::new());

    t.engine.handle_command(Command::DeleteProfile { name: "prod".to_string() });

    assert!(saved(&t).is_empty());
    assert!(matches!(t.drain().as_slice(), [Event::Profiles(entries)] if entries.is_empty()));
}

#[test]
fn listing_profiles_sends_every_saved_profile() {
    let mut t = engine_with_sftp();
    seed(&t, "b", "b.example.com", BTreeMap::new());
    seed(&t, "a", "a.example.com", BTreeMap::new());

    t.engine.handle_command(Command::ListProfiles);

    match t.drain().as_slice() {
        [Event::Profiles(entries)] => {
            assert_eq!(entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn saving_a_draft_for_an_unknown_protocol_is_rejected() {
    let mut t = engine_with_sftp();
    let mut unknown = draft("web", "h", "22");
    unknown.protocol = "gopher".to_string();

    t.engine.handle_command(Command::SaveProfile { original: None, draft: unknown });

    assert!(matches!(
        t.drain().as_slice(),
        [Event::ProfileRejected { message }] if message == "No \"gopher\" protocol is available"
    ));
    assert!(saved(&t).is_empty());
}

#[test]
fn saving_validates_against_the_protocols_own_form() {
    let mut t = engine_with_sftp();
    let mut with_key = draft("web", "h", "");
    with_key.options.insert("identity_file".to_string(), " /k ".to_string());

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_key });

    let profiles = saved(&t);
    assert_eq!(profiles[0].port, Some(22));
    assert_eq!(profiles[0].options.get("identity_file").map(String::as_str), Some("/k"));
}

#[test]
fn editing_keeps_hand_written_options_of_the_same_protocol() {
    let mut t = engine_with_sftp();
    seed(&t, "web", "h", BTreeMap::from([("foo".to_string(), "bar".to_string())]));

    t.engine.handle_command(Command::SaveProfile { original: Some("web".into()), draft: draft("web", "h2", "22") });

    assert_eq!(saved(&t)[0].options.get("foo").map(String::as_str), Some("bar"));
}

#[test]
fn editing_an_entry_of_a_protocol_missing_from_this_build_keeps_its_options() {
    let mut t = engine_with_sftp();
    store::save(
        &t.engine.paths,
        &ConnectionProfile {
            name: "files".to_string(),
            protocol: "ftp".to_string(),
            host: "h".to_string(),
            port: Some(21),
            username: "u".to_string(),
            password: None,
            options: BTreeMap::from([("security".to_string(), "explicit".to_string())]),
        },
    )
    .unwrap();
    let mut edit = draft("files", "h2", "21");
    edit.protocol = "ftp".to_string();

    t.engine.handle_command(Command::SaveProfile { original: Some("files".into()), draft: edit });

    let profiles = saved(&t);
    assert_eq!((profiles[0].protocol.as_str(), profiles[0].host.as_str()), ("ftp", "h2"));
    assert_eq!(profiles[0].options.get("security").map(String::as_str), Some("explicit"));
}
