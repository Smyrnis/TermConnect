use std::collections::BTreeMap;

use super::*;
use crate::{
    engine::{
        Command,
        testing::{TestEngine, test_engine, test_engine_with_secrets},
    },
    profiles::{ConnectionProfile, Labels, SecretEdit},
    secrets::TestBackend,
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
            options,
            group: None,
            tags: Vec::new(),
            in_keyring: Vec::new(),
        },
    )
    .unwrap();
}

#[tokio::test]
async fn saving_a_new_profile_stores_it_and_lists_the_profiles() {
    let mut t = engine_with_sftp();
    let mut new = draft("prod", "server.example.com", "2222");
    new.password = SecretEdit::Replace("hunter2".into());

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(new) });

    let events = t.drain();
    assert!(matches!(events[0], Event::ProfileSaved));
    assert!(matches!(&events[1], Event::Profiles(entries) if entries.len() == 1));
    let saved = saved(&t);
    assert_eq!(
        (saved[0].name.as_str(), saved[0].host.as_str(), saved[0].port),
        ("prod", "server.example.com", Some(2222))
    );
    t.settle().await;
    let events = t.drain();
    t.assert_secret_nowhere("hunter2", &events);
}

#[test]
fn an_invalid_draft_is_rejected_with_the_reason() {
    let mut t = engine_with_sftp();

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(draft("prod", "h", "not-a-port")) });

    assert!(
        matches!(t.drain().as_slice(), [Event::ProfileRejected { message }] if message == "Port must be a number from 1-65535")
    );
    assert!(saved(&t).is_empty());
}

#[test]
fn add_connection_dialog_rejects_a_name_that_already_exists() {
    let mut t = engine_with_sftp();
    seed(&t, "prod", "original.example.com", BTreeMap::new());

    t.engine.handle_command(Command::SaveProfile {
        original: None,
        draft: Box::new(draft("prod", "new.example.com", "22")),
    });

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
        draft: Box::new(draft("staging", "prod.example.com", "22")),
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

    t.engine.handle_command(Command::SaveProfile { original: Some("prod".to_string()), draft: Box::new(edit) });

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
        draft: Box::new(draft("production", "server.example.com", "22")),
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

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(draft("prod", "h", "22")) });

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

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(unknown) });

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

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(with_key) });

    let profiles = saved(&t);
    assert_eq!(profiles[0].port, Some(22));
    assert_eq!(profiles[0].options.get("identity_file").map(String::as_str), Some("/k"));
}

#[test]
fn editing_keeps_hand_written_options_of_the_same_protocol() {
    let mut t = engine_with_sftp();
    seed(&t, "web", "h", BTreeMap::from([("foo".to_string(), "bar".to_string())]));

    t.engine.handle_command(Command::SaveProfile {
        original: Some("web".into()),
        draft: Box::new(draft("web", "h2", "22")),
    });

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
            options: BTreeMap::from([("security".to_string(), "explicit".to_string())]),
            group: None,
            tags: Vec::new(),
            in_keyring: Vec::new(),
        },
    )
    .unwrap();
    let mut edit = draft("files", "h2", "21");
    edit.protocol = "ftp".to_string();

    t.engine.handle_command(Command::SaveProfile { original: Some("files".into()), draft: Box::new(edit) });

    let profiles = saved(&t);
    assert_eq!((profiles[0].protocol.as_str(), profiles[0].host.as_str()), ("ftp", "h2"));
    assert_eq!(profiles[0].options.get("security").map(String::as_str), Some("explicit"));
}

fn discovered(name: &str) -> porthmos_vfs::Target {
    porthmos_vfs::Target {
        name: name.to_string(),
        host: format!("{name}.example"),
        port: 22,
        username: "u".into(),
        password: None,
        options: Default::default(),
    }
}

fn engine_discovering(names: &[&str]) -> TestEngine {
    let mut t = test_engine();
    let protocol = porthmos_vfs::testing::FakeProtocol::new(porthmos_vfs::testing::FakeFs::new())
        .with_id("sftp")
        .with_discovered(names.iter().map(|name| discovered(name)).collect());
    t.engine.protocols = vec![std::sync::Arc::new(protocol)];
    t
}

fn group_only(group: &str) -> Labels {
    Labels { group: Some(group.to_string()), tags: Vec::new(), in_keyring: Vec::new() }
}

#[test]
fn saving_ssh_labels_stores_them_normalized_and_relists() {
    let mut t = engine_discovering(&["web1"]);

    t.engine.handle_command(Command::SaveSshLabels {
        name: "web1".into(),
        group: " Work / Web ".into(),
        tags: "prod, prod".into(),
    });

    let labels = store::load_ssh_labels(&t.engine.paths).unwrap();
    assert_eq!(labels.get("web1").unwrap().group.as_deref(), Some("Work/Web"));
    assert_eq!(labels.get("web1").unwrap().tags, vec!["prod"]);
    let events = t.drain();
    assert!(matches!(events[0], Event::ProfileSaved));
    assert!(matches!(&events[1], Event::Profiles(entries) if entries[0].group.as_deref() == Some("Work/Web")));
}

#[test]
fn saving_ssh_labels_for_an_unknown_host_is_rejected() {
    let mut t = engine_discovering(&["web1"]);

    t.engine.handle_command(Command::SaveSshLabels { name: "nope".into(), group: "A".into(), tags: String::new() });

    assert!(
        matches!(t.drain().as_slice(), [Event::ProfileRejected { message }] if message == "nope is not in ~/.ssh/config")
    );
    assert!(store::load_ssh_labels(&t.engine.paths).unwrap().is_empty());
}

#[test]
fn moving_ssh_labels_reattaches_them_to_another_host() {
    let mut t = engine_discovering(&["new"]);
    store::save_ssh_labels(&t.engine.paths, "old", &group_only("A")).unwrap();

    t.engine.handle_command(Command::MoveSshLabels { from: "old".into(), to: "new".into() });

    let labels = store::load_ssh_labels(&t.engine.paths).unwrap();
    assert_eq!(labels.keys().collect::<Vec<_>>(), vec!["new"]);
    assert!(matches!(t.drain().last(), Some(Event::Profiles(_))));
}

#[test]
fn moving_ssh_labels_to_an_unknown_host_is_an_error_notice() {
    let mut t = engine_discovering(&["new"]);
    store::save_ssh_labels(&t.engine.paths, "old", &group_only("A")).unwrap();

    t.engine.handle_command(Command::MoveSshLabels { from: "old".into(), to: "ghost".into() });

    assert!(t.drain().iter().any(|event| matches!(
        event,
        Event::Notice { severity: Severity::Error, message } if message == "ghost is not in ~/.ssh/config"
    )));
    assert!(store::load_ssh_labels(&t.engine.paths).unwrap().contains_key("old"));
}

#[test]
fn forgetting_ssh_labels_removes_them_and_relists() {
    let mut t = engine_discovering(&[]);
    store::save_ssh_labels(&t.engine.paths, "old", &group_only("A")).unwrap();

    t.engine.handle_command(Command::ForgetSshLabels { name: "old".into() });

    assert!(store::load_ssh_labels(&t.engine.paths).unwrap().is_empty());
    assert!(matches!(t.drain().last(), Some(Event::Profiles(entries)) if entries.is_empty()));
}

#[test]
fn a_saved_profile_carries_its_group_and_tags() {
    let mut t = engine_with_sftp();
    let mut new = draft("web", "h", "22");
    new.group = "Work".into();
    new.tags = "prod".into();

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(new) });

    let profile = saved(&t).remove(0);
    assert_eq!((profile.group.as_deref(), profile.tags), (Some("Work"), vec!["prod".to_string()]));
}

fn secured_form() -> porthmos_vfs::ConnectionForm {
    let mut form = porthmos_vfs::ConnectionForm::standard(22);
    form.options.push(porthmos_vfs::OptionField {
        key: "token",
        label: "Token",
        required: false,
        kind: porthmos_vfs::OptionKind::Secret,
    });
    form
}

fn engine_with_sftp_and(backend: &std::sync::Arc<TestBackend>) -> TestEngine {
    let mut t = test_engine_with_secrets(backend.clone());
    let protocol = porthmos_vfs::testing::FakeProtocol::new(porthmos_vfs::testing::FakeFs::new())
        .with_id("sftp")
        .with_form(secured_form());
    t.engine.protocols = vec![std::sync::Arc::new(protocol)];
    t
}

fn with_password(name: &str, password: &str) -> Box<ProfileDraft> {
    let mut draft = draft(name, "h", "22");
    draft.password = SecretEdit::Replace(password.into());
    Box::new(draft)
}

fn seed_with_markers(t: &TestEngine, name: &str, markers: &[&str]) {
    seed(t, name, "h", BTreeMap::new());
    store::set_profile_markers(&t.engine.paths, name, &markers.iter().map(|m| m.to_string()).collect::<Vec<_>>())
        .unwrap();
}

fn markers_of(t: &TestEngine, name: &str) -> Vec<String> {
    saved(t).into_iter().find(|profile| profile.name == name).map(|profile| profile.in_keyring).unwrap_or_default()
}

fn error_notices(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Notice { severity: Severity::Error, message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_saved_password_goes_to_the_keyring_and_only_a_marker_to_the_file() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "hunter2") });
    t.settle().await;

    assert_eq!(backend.stored("profile:web").as_deref(), Some("hunter2"));
    assert_eq!(backend.calls(), vec!["set profile:web"]);
    assert_eq!(markers_of(&t, "web"), vec!["password"]);
    let events = t.drain();
    assert!(error_notices(&events).is_empty());
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Profiles(entries) if entries.iter().any(|entry| entry.name == "web" && entry.saved_password)
    )));
    t.assert_secret_nowhere("hunter2", &events);
}

#[tokio::test]
async fn the_form_closes_before_the_keyring_answers() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.block_for(std::time::Duration::from_millis(100));
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw") });

    let events = t.drain();
    assert!(matches!(events.as_slice(), [Event::ProfileSaved, Event::Profiles(_)]), "{events:?}");
    t.settle().await;
}

#[tokio::test]
async fn a_failing_keyring_write_saves_the_profile_without_a_marker_and_says_so() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.fail_writes();
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw") });
    t.settle().await;

    assert!(markers_of(&t, "web").is_empty());
    let events = t.drain();
    assert_eq!(
        error_notices(&events),
        vec!["Couldn't save the password for web in the system keyring: write failed".to_string()]
    );
    assert_eq!(t.engine.secrets.cached("profile:web").as_deref().map(String::as_str), Some("pw"));
    t.assert_secret_nowhere("pw\"", &events);
}

#[tokio::test]
async fn keep_leaves_the_keyring_alone_and_clear_removes_the_entry() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    let mut t = engine_with_sftp_and(&backend);
    seed_with_markers(&t, "web", &["password"]);

    t.engine.handle_command(Command::SaveProfile {
        original: Some("web".into()),
        draft: Box::new(draft("web", "h2", "22")),
    });
    t.settle().await;
    assert!(backend.calls().is_empty());
    assert_eq!(markers_of(&t, "web"), vec!["password"]);
    assert_eq!(saved(&t)[0].host, "h2");

    let mut clear = draft("web", "h2", "22");
    clear.password = SecretEdit::Clear;
    t.engine.handle_command(Command::SaveProfile { original: Some("web".into()), draft: Box::new(clear) });
    t.settle().await;
    assert!(backend.stored("profile:web").is_none());
    assert_eq!(backend.calls(), vec!["delete profile:web"]);
    assert!(markers_of(&t, "web").is_empty());
}

#[tokio::test]
async fn clearing_without_a_saved_password_never_touches_the_keyring() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);
    let mut clear = draft("web", "h", "22");
    clear.password = SecretEdit::Clear;

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(clear) });
    t.settle().await;

    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn renaming_moves_the_secret_and_deleting_forgets_it() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("profile:old", "pw");
    let mut t = engine_with_sftp_and(&backend);
    seed_with_markers(&t, "old", &["password"]);

    t.engine.handle_command(Command::SaveProfile {
        original: Some("old".into()),
        draft: Box::new(draft("new", "h", "22")),
    });
    t.settle().await;
    assert!(backend.stored("profile:old").is_none());
    assert_eq!(backend.stored("profile:new").as_deref(), Some("pw"));
    assert_eq!(markers_of(&t, "new"), vec!["password"]);
    assert_eq!(backend.calls(), vec!["get profile:old", "set profile:new", "delete profile:old"]);

    t.engine.handle_command(Command::DeleteProfile { name: "new".into() });
    t.settle().await;
    assert!(backend.stored("profile:new").is_none());
    assert_eq!(backend.calls().last().map(String::as_str), Some("delete profile:new"));
    assert!(error_notices(&t.drain()).is_empty());
}

#[tokio::test]
async fn renaming_without_saved_secrets_never_touches_the_keyring() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);
    seed(&t, "old", "h", BTreeMap::new());

    t.engine.handle_command(Command::SaveProfile {
        original: Some("old".into()),
        draft: Box::new(draft("new", "h", "22")),
    });
    t.engine.handle_command(Command::DeleteProfile { name: "new".into() });
    t.settle().await;

    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn renaming_and_replacing_at_once_leaves_only_the_new_password_under_the_new_name() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("profile:old", "old-pw");
    let mut t = engine_with_sftp_and(&backend);
    seed_with_markers(&t, "old", &["password"]);
    let mut edit = draft("new", "h", "22");
    edit.password = SecretEdit::Replace("new-pw".into());

    t.engine.handle_command(Command::SaveProfile { original: Some("old".into()), draft: Box::new(edit) });
    t.settle().await;

    assert!(backend.stored("profile:old").is_none());
    assert_eq!(backend.stored("profile:new").as_deref(), Some("new-pw"));
    assert_eq!(markers_of(&t, "new"), vec!["password"]);
    let events = t.drain();
    t.assert_secret_nowhere("new-pw", &events);
    t.assert_secret_nowhere("old-pw", &events);
}

#[tokio::test]
async fn a_late_secret_result_for_a_renamed_profile_lands_on_the_new_name() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.block_for(std::time::Duration::from_millis(50));
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw") });
    t.engine.handle_command(Command::SaveProfile {
        original: Some("web".into()),
        draft: Box::new(draft("site", "h", "22")),
    });
    t.settle().await;

    let names: Vec<String> = saved(&t).into_iter().map(|profile| profile.name).collect();
    assert_eq!(names, vec!["site"]);
    assert_eq!(markers_of(&t, "site"), vec!["password"]);
    assert_eq!(backend.stored("profile:site").as_deref(), Some("pw"));
    assert!(backend.stored("profile:web").is_none());
    assert_eq!(t.engine.secrets.cached("profile:site").as_deref().map(String::as_str), Some("pw"));
    assert!(t.engine.secrets.cached("profile:web").is_none());
}

#[tokio::test]
async fn a_profile_deleted_before_its_secret_was_stored_leaves_nothing_behind() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.block_for(std::time::Duration::from_millis(50));
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw") });
    t.engine.handle_command(Command::DeleteProfile { name: "web".into() });
    t.settle().await;

    assert!(saved(&t).is_empty());
    assert!(backend.stored("profile:web").is_none());
    assert!(t.engine.secrets.cached("profile:web").is_none());
}

#[tokio::test]
async fn a_secret_option_is_stored_under_its_own_account() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);
    let mut new = draft("web", "h", "22");
    new.secret_options.insert("token".into(), SecretEdit::Replace("tok-123".into()));

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(new) });
    t.settle().await;

    assert_eq!(backend.stored("option:token:web").as_deref(), Some("tok-123"));
    assert!(backend.stored("profile:web").is_none());
    assert_eq!(markers_of(&t, "web"), vec!["token"]);
    assert!(!saved(&t)[0].options.contains_key("token"));
    let events = t.drain();
    t.assert_secret_nowhere("tok-123", &events);
}

#[tokio::test]
async fn a_rejected_draft_never_touches_the_keyring() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);
    let mut bad = draft("web", "h", "not-a-port");
    bad.password = SecretEdit::Replace("pw".into());

    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(bad) });
    t.settle().await;

    assert!(backend.calls().is_empty());
    assert!(t.engine.secrets.cached("profile:web").is_none());
}

#[tokio::test]
async fn without_a_keyring_a_typed_password_is_kept_for_the_run_only() {
    let mut t = engine_with_sftp();

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "hunter2") });
    t.settle().await;

    assert!(markers_of(&t, "web").is_empty());
    assert_eq!(t.engine.secrets.cached("profile:web").as_deref().map(String::as_str), Some("hunter2"));
    let events = t.drain();
    assert!(error_notices(&events).is_empty());
    t.assert_secret_nowhere("hunter2", &events);
}

#[tokio::test]
async fn a_password_is_stored_exactly_as_typed() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", " p w: ä ") });
    t.settle().await;

    assert_eq!(backend.stored("profile:web").as_deref(), Some(" p w: ä "));
}

#[tokio::test]
async fn a_failing_delete_reports_it_and_still_removes_the_profile() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    let mut t = engine_with_sftp_and(&backend);
    seed_with_markers(&t, "web", &["password"]);
    backend.fail_writes();

    t.engine.handle_command(Command::DeleteProfile { name: "web".into() });
    t.settle().await;

    assert!(saved(&t).is_empty());
    assert_eq!(
        error_notices(&t.drain()),
        vec!["Couldn't remove the password for web from the system keyring: write failed".to_string()]
    );
}

#[test]
fn a_save_command_never_shows_the_password_when_printed() {
    let command = Command::SaveProfile { original: None, draft: with_password("web", "hunter2") };
    assert!(!format!("{command:?}").contains("hunter2"));
}

#[tokio::test]
async fn switching_protocol_forgets_the_old_protocols_saved_secrets() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("profile:web", "pw");
    backend.put("option:token:web", "tok");
    let mut t = engine_with_sftp_and(&backend);
    t.engine.protocols.push(std::sync::Arc::new(
        porthmos_vfs::testing::FakeProtocol::new(porthmos_vfs::testing::FakeFs::new()).with_id("ftp"),
    ));
    seed_with_markers(&t, "web", &["password", "token"]);
    let mut switched = draft("web", "h", "21");
    switched.protocol = "ftp".into();

    t.engine.handle_command(Command::SaveProfile { original: Some("web".into()), draft: Box::new(switched) });
    t.settle().await;

    assert!(backend.stored("profile:web").is_none());
    assert!(backend.stored("option:token:web").is_none());
    assert!(markers_of(&t, "web").is_empty());
    assert_eq!(saved(&t)[0].protocol, "ftp");
}

fn engine_discovering_with(backend: &std::sync::Arc<TestBackend>, names: &[&str]) -> TestEngine {
    let mut t = test_engine_with_secrets(backend.clone());
    let protocol = porthmos_vfs::testing::FakeProtocol::new(porthmos_vfs::testing::FakeFs::new())
        .with_id("sftp")
        .with_discovered(names.iter().map(|name| discovered(name)).collect());
    t.engine.protocols = vec![std::sync::Arc::new(protocol)];
    t
}

fn mark_ssh(t: &TestEngine, alias: &str) {
    store::set_ssh_markers(&t.engine.paths, alias, &["password".to_string()]).unwrap();
}

#[tokio::test]
async fn moving_labels_moves_the_saved_password_too() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("ssh:old", "pw");
    let mut t = engine_discovering_with(&backend, &["new"]);
    store::save_ssh_labels(&t.engine.paths, "old", &group_only("Work")).unwrap();
    mark_ssh(&t, "old");
    t.engine.secrets.remember("ssh:old", "pw");

    t.engine.handle_command(Command::MoveSshLabels { from: "old".into(), to: "new".into() });
    t.settle().await;

    assert_eq!(backend.stored("ssh:new").as_deref(), Some("pw"));
    assert!(backend.stored("ssh:old").is_none());
    assert_eq!(backend.calls(), vec!["get ssh:old", "set ssh:new", "delete ssh:old"]);
    let records = store::load_ssh_labels(&t.engine.paths).unwrap();
    assert_eq!(records["new"].in_keyring, vec!["password"]);
    assert_eq!(records["new"].group.as_deref(), Some("Work"));
    assert!(!records.contains_key("old"));
    assert_eq!(t.engine.secrets.cached("ssh:new").as_deref().map(String::as_str), Some("pw"));
    assert!(t.engine.secrets.cached("ssh:old").is_none());
}

#[tokio::test]
async fn moving_labels_without_a_saved_password_never_touches_the_keyring() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_discovering_with(&backend, &["new"]);
    store::save_ssh_labels(&t.engine.paths, "old", &group_only("Work")).unwrap();

    t.engine.handle_command(Command::MoveSshLabels { from: "old".into(), to: "new".into() });
    t.settle().await;

    assert!(backend.calls().is_empty());
    assert!(store::load_ssh_labels(&t.engine.paths).unwrap()["new"].in_keyring.is_empty());
}

#[tokio::test]
async fn a_failed_password_move_is_reported_and_leaves_no_false_marker() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("ssh:old", "pw");
    backend.fail_writes();
    let mut t = engine_discovering_with(&backend, &["new"]);
    store::save_ssh_labels(&t.engine.paths, "old", &group_only("Work")).unwrap();
    mark_ssh(&t, "old");

    t.engine.handle_command(Command::MoveSshLabels { from: "old".into(), to: "new".into() });
    t.settle().await;

    let records = store::load_ssh_labels(&t.engine.paths).unwrap();
    assert!(records["new"].in_keyring.is_empty());
    assert_eq!(records["new"].group.as_deref(), Some("Work"));
    assert_eq!(backend.stored("ssh:old").as_deref(), Some("pw"));
    assert_eq!(
        error_notices(&t.drain()),
        vec!["Couldn't save the password for new in the system keyring: write failed".to_string()]
    );
}

#[tokio::test]
async fn moving_labels_to_an_unknown_host_leaves_the_password_alone() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("ssh:old", "pw");
    let mut t = engine_discovering_with(&backend, &["new"]);
    mark_ssh(&t, "old");

    t.engine.handle_command(Command::MoveSshLabels { from: "old".into(), to: "ghost".into() });
    t.settle().await;

    assert!(backend.calls().is_empty());
    assert_eq!(store::load_ssh_labels(&t.engine.paths).unwrap()["old"].in_keyring, vec!["password"]);
}

#[tokio::test]
async fn forgetting_labels_forgets_the_password() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("ssh:old", "pw");
    let mut t = engine_discovering_with(&backend, &[]);
    mark_ssh(&t, "old");
    t.engine.secrets.remember("ssh:old", "pw");

    t.engine.handle_command(Command::ForgetSshLabels { name: "old".into() });
    t.settle().await;

    assert!(backend.stored("ssh:old").is_none());
    assert_eq!(backend.calls(), vec!["delete ssh:old"]);
    assert!(store::load_ssh_labels(&t.engine.paths).unwrap().is_empty());
    assert!(t.engine.secrets.cached("ssh:old").is_none());
}

#[tokio::test]
async fn forgetting_labels_without_a_saved_password_never_touches_the_keyring() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_discovering_with(&backend, &[]);
    store::save_ssh_labels(&t.engine.paths, "old", &group_only("Work")).unwrap();

    t.engine.handle_command(Command::ForgetSshLabels { name: "old".into() });
    t.settle().await;

    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn forgetting_only_the_password_keeps_group_and_tags() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("ssh:web1", "pw");
    let mut t = engine_discovering_with(&backend, &["web1"]);
    store::save_ssh_labels(&t.engine.paths, "web1", &group_only("Work")).unwrap();
    mark_ssh(&t, "web1");
    t.engine.secrets.remember("ssh:web1", "pw");

    t.engine.handle_command(Command::ForgetSshPassword { alias: "web1".into() });
    t.settle().await;

    let record = &store::load_ssh_labels(&t.engine.paths).unwrap()["web1"];
    assert_eq!(record.group.as_deref(), Some("Work"));
    assert!(record.in_keyring.is_empty());
    assert!(backend.stored("ssh:web1").is_none());
    assert!(t.engine.secrets.cached("ssh:web1").is_none());
    assert!(t.drain().iter().any(|event| matches!(
        event,
        Event::Profiles(entries) if entries.iter().any(|entry| entry.name == "web1" && !entry.saved_password)
    )));
}

#[tokio::test]
async fn forgetting_a_password_that_was_never_saved_only_clears_the_run_copy() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_discovering_with(&backend, &["web1"]);
    t.engine.secrets.remember("ssh:web1", "pw");

    t.engine.handle_command(Command::ForgetSshPassword { alias: "web1".into() });
    t.settle().await;

    assert!(backend.calls().is_empty());
    assert!(t.engine.secrets.cached("ssh:web1").is_none());
}

#[tokio::test]
async fn a_failed_password_forget_is_reported() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.put("ssh:web1", "pw");
    backend.fail_writes();
    let mut t = engine_discovering_with(&backend, &["web1"]);
    mark_ssh(&t, "web1");

    t.engine.handle_command(Command::ForgetSshPassword { alias: "web1".into() });
    t.settle().await;

    assert_eq!(
        error_notices(&t.drain()),
        vec!["Couldn't remove the password for web1 from the system keyring: write failed".to_string()]
    );
    assert_eq!(store::load_ssh_labels(&t.engine.paths).unwrap()["web1"].in_keyring, vec!["password"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reusing_a_name_after_a_rename_keeps_both_passwords_apart() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.block_for(std::time::Duration::from_millis(20));
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw-web") });
    t.settle().await;
    t.engine.handle_command(Command::SaveProfile {
        original: Some("web".into()),
        draft: Box::new(draft("site", "h", "22")),
    });
    let mut again = draft("web", "other-host", "22");
    again.password = SecretEdit::Replace("pw-new".into());
    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(again) });
    t.settle().await;

    assert_eq!(backend.stored("profile:site").as_deref(), Some("pw-web"));
    assert_eq!(backend.stored("profile:web").as_deref(), Some("pw-new"));
    assert_eq!(markers_of(&t, "site"), vec!["password"]);
    assert_eq!(markers_of(&t, "web"), vec!["password"]);
}

#[tokio::test]
async fn deleting_a_reused_name_leaves_the_renamed_profile_alone() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);
    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw-web") });
    t.settle().await;
    t.engine.handle_command(Command::SaveProfile {
        original: Some("web".into()),
        draft: Box::new(draft("site", "h", "22")),
    });
    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw-new") });
    t.settle().await;
    t.engine.handle_command(Command::DeleteProfile { name: "web".into() });
    t.settle().await;

    assert_eq!(markers_of(&t, "site"), vec!["password"]);
    assert!(backend.stored("profile:web").is_none());
    assert_eq!(backend.stored("profile:site").as_deref(), Some("pw-web"));
}

#[tokio::test]
async fn a_keep_edit_while_a_replace_is_in_flight_keeps_the_marker() {
    let backend = std::sync::Arc::new(TestBackend::new());
    backend.block_for(std::time::Duration::from_millis(60));
    let mut t = engine_with_sftp_and(&backend);

    t.engine.handle_command(Command::SaveProfile { original: None, draft: with_password("web", "pw") });
    let mut token_only = draft("web", "h", "22");
    token_only.secret_options.insert("token".into(), SecretEdit::Replace("tok".into()));
    t.engine.handle_command(Command::SaveProfile { original: Some("web".into()), draft: Box::new(token_only) });
    t.settle().await;

    let mut markers = markers_of(&t, "web");
    markers.sort();
    assert_eq!(markers, vec!["password", "token"]);
    assert_eq!(backend.stored("profile:web").as_deref(), Some("pw"));
    assert_eq!(backend.stored("option:token:web").as_deref(), Some("tok"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn renaming_there_and_back_quickly_keeps_the_password() {
    for _ in 0..20 {
        let backend = std::sync::Arc::new(TestBackend::new());
        backend.put("profile:a", "pw");
        backend.block_for(std::time::Duration::from_millis(15));
        let mut t = engine_with_sftp_and(&backend);
        seed_with_markers(&t, "a", &["password"]);

        t.engine.handle_command(Command::SaveProfile {
            original: Some("a".into()),
            draft: Box::new(draft("b", "h", "22")),
        });
        t.engine.handle_command(Command::SaveProfile {
            original: Some("b".into()),
            draft: Box::new(draft("a", "h", "22")),
        });
        t.settle().await;

        assert_eq!(backend.stored("profile:a").as_deref(), Some("pw"));
        assert!(backend.stored("profile:b").is_none());
        assert_eq!(markers_of(&t, "a"), vec!["password"]);
    }
}

#[tokio::test]
async fn a_cached_only_password_follows_a_rename_and_is_not_given_to_a_new_profile() {
    let mut t = engine_with_sftp();
    t.engine.secrets.remember("profile:web", "typed-at-prompt");
    seed(&t, "web", "h", BTreeMap::new());

    t.engine.handle_command(Command::SaveProfile {
        original: Some("web".into()),
        draft: Box::new(draft("site", "h", "22")),
    });
    t.engine.handle_command(Command::SaveProfile { original: None, draft: Box::new(draft("web", "other-host", "22")) });

    assert!(t.engine.secrets.cached("profile:web").is_none());
    assert_eq!(t.engine.secrets.cached("profile:site").as_deref().map(String::as_str), Some("typed-at-prompt"));
}

#[tokio::test]
async fn deleting_forgets_every_cached_secret_of_the_profile() {
    let mut t = engine_with_sftp();
    seed(&t, "web", "h", BTreeMap::new());
    t.engine.secrets.remember("profile:web", "pw");
    t.engine.secrets.remember("option:token:web", "tok");
    t.engine.secrets.remember("profile:other", "keep");

    t.engine.handle_command(Command::DeleteProfile { name: "web".into() });

    assert!(t.engine.secrets.cached("profile:web").is_none());
    assert!(t.engine.secrets.cached("option:token:web").is_none());
    assert!(t.engine.secrets.cached("profile:other").is_some());
}

#[tokio::test]
async fn keeping_a_password_whose_keyring_entry_was_deleted_by_hand_is_quiet() {
    let backend = std::sync::Arc::new(TestBackend::new());
    let mut t = engine_with_sftp_and(&backend);
    seed_with_markers(&t, "web", &["password"]);

    t.engine.handle_command(Command::SaveProfile {
        original: Some("web".into()),
        draft: Box::new(draft("web", "h2", "22")),
    });
    t.settle().await;

    assert!(backend.calls().is_empty());
    assert!(error_notices(&t.drain()).is_empty());
    assert_eq!(markers_of(&t, "web"), vec!["password"]);
    assert!(backend.stored("profile:web").is_none());
}
