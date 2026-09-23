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
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: Some("hunter2".to_string()),
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
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: Some("hunter2".to_string()),
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
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: Some("hunter2".to_string()),
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
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: Some("/var/www".to_string()),
        password: Some("hunter2".to_string()),
    };

    let entry: ConnectionEntry = profile.into();

    assert_eq!(entry.source, ConnectionSource::Profile);
    assert_eq!(entry.password, Some("hunter2".to_string()));
    assert_eq!(entry.remote_path, Some("/var/www".to_string()));
}
