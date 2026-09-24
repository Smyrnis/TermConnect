use std::collections::BTreeMap;

use super::*;

#[test]
fn deserializing_a_profile_without_a_password_defaults_to_none() {
    let toml = r#"
host = "server.example.com"
username = "deploy"
"#;
    let profile: ConnectionProfile = toml::from_str(toml).unwrap();
    assert_eq!(profile.password, None);
}

#[test]
fn serializing_a_profile_round_trips_the_password() {
    let profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: Some(22),
        username: "deploy".to_string(),
        password: Some("hunter2".to_string()),
        options: BTreeMap::new(),
    };

    let serialized = toml::to_string(&profile).unwrap();
    let deserialized: ConnectionProfile = toml::from_str(&serialized).unwrap();

    assert_eq!(deserialized.password, Some("hunter2".to_string()));
}

#[test]
fn debug_formatting_a_profile_redacts_the_password() {
    let profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: Some(22),
        username: "deploy".to_string(),
        password: Some("hunter2".to_string()),
        options: BTreeMap::new(),
    };

    let debug_output = format!("{profile:?}");

    assert!(!debug_output.contains("hunter2"));
    assert!(debug_output.contains("server.example.com"));
}

#[test]
fn debug_formatting_an_entry_redacts_the_password() {
    let entry = ConnectionEntry {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: 22,
        username: "deploy".to_string(),
        password: Some("hunter2".to_string()),
        options: BTreeMap::new(),
        source: ConnectionSource::Profile,
    };

    let debug_output = format!("{entry:?}");

    assert!(!debug_output.contains("hunter2"));
    assert!(debug_output.contains("server.example.com"));
}

#[test]
fn converting_a_profile_to_an_entry_tags_it_as_profile_sourced() {
    let profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: None,
        username: "deploy".to_string(),
        password: Some("hunter2".to_string()),
        options: BTreeMap::from([("remote_path".to_string(), "/var/www".to_string())]),
    };

    let entry = ConnectionEntry::from_profile(profile, 22);

    assert_eq!(entry.source, ConnectionSource::Profile);
    assert_eq!(entry.password, Some("hunter2".to_string()));
    assert_eq!(entry.port, 22);
    assert_eq!(entry.option("remote_path"), Some("/var/www"));
}

#[test]
fn a_profile_without_a_protocol_is_sftp() {
    let profile: ConnectionProfile = toml::from_str("host = \"h\"\nusername = \"u\"\n").unwrap();

    assert_eq!(profile.protocol, "sftp");
}

#[test]
fn the_default_protocol_is_not_written_back() {
    let profile: ConnectionProfile = toml::from_str("host = \"h\"\nusername = \"u\"\n").unwrap();

    assert!(!toml::to_string(&profile).unwrap().contains("protocol"));
}

#[test]
fn another_protocol_is_written_back() {
    let profile: ConnectionProfile = toml::from_str("protocol = \"ftp\"\nhost = \"h\"\nusername = \"u\"\n").unwrap();

    assert!(toml::to_string(&profile).unwrap().contains("protocol = \"ftp\""));
}

#[test]
fn an_entry_hands_its_fields_and_options_to_the_protocol_target() {
    let entry = ConnectionEntry {
        name: "web".to_string(),
        protocol: "sftp".to_string(),
        host: "example.com".to_string(),
        port: 2222,
        username: "deploy".to_string(),
        password: Some("pw".to_string()),
        options: BTreeMap::from([("identity_file".to_string(), "/k".to_string())]),
        source: ConnectionSource::Profile,
    };

    let target = entry.target();

    assert_eq!((target.name.as_str(), target.host.as_str(), target.port), ("web", "example.com", 2222));
    assert_eq!(target.password.as_deref(), Some("pw"));
    assert_eq!(target.option("identity_file"), Some("/k"));
}
