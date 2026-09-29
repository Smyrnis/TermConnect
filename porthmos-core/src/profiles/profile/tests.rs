use std::collections::BTreeMap;

use super::*;
use crate::profiles::Labels;

#[test]
fn serializing_a_profile_round_trips_every_field() {
    let profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: Some(22),
        username: "deploy".to_string(),
        options: BTreeMap::new(),
        group: None,
        tags: Vec::new(),
        in_keyring: Vec::new(),
    };

    let serialized = toml::to_string(&profile).unwrap();
    let deserialized: ConnectionProfile = toml::from_str(&serialized).unwrap();

    assert_eq!(deserialized, ConnectionProfile { name: String::new(), ..profile });
}

#[test]
fn debug_formatting_a_profile_redacts_the_password() {
    let profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: Some(22),
        username: "deploy".to_string(),
        options: BTreeMap::new(),
        group: None,
        tags: Vec::new(),
        in_keyring: Vec::new(),
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
        group: None,
        tags: Vec::new(),
        saved_password: false,
        in_keyring: Vec::new(),
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
        options: BTreeMap::from([("remote_path".to_string(), "/var/www".to_string())]),
        group: None,
        tags: Vec::new(),
        in_keyring: Vec::new(),
    };

    let entry = ConnectionEntry::from_profile(profile, 22);

    assert_eq!(entry.source, ConnectionSource::Profile);
    assert_eq!(entry.password, None);
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
        group: None,
        tags: Vec::new(),
        saved_password: false,
        in_keyring: Vec::new(),
    };

    let target = entry.target();

    assert_eq!((target.name.as_str(), target.host.as_str(), target.port), ("web", "example.com", 2222));
    assert_eq!(target.password.as_deref(), Some("pw"));
    assert_eq!(target.option("identity_file"), Some("/k"));
}

fn entry_with_options(options: &[(&str, &str)]) -> ConnectionEntry {
    ConnectionEntry::from_profile(
        ConnectionProfile {
            name: "prod".to_string(),
            host: "server.example.com".to_string(),
            protocol: "sftp".to_string(),
            port: None,
            username: "deploy".to_string(),
            options: options.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect(),
            group: None,
            tags: Vec::new(),
            in_keyring: Vec::new(),
        },
        22,
    )
}

#[test]
fn the_start_path_is_the_profiles_remote_path() {
    assert_eq!(entry_with_options(&[("remote_path", "/var/www")]).start_path(), Some("/var/www"));
}

#[test]
fn the_start_path_ignores_surrounding_whitespace() {
    assert_eq!(entry_with_options(&[("remote_path", "  /var/www \t")]).start_path(), Some("/var/www"));
}

#[test]
fn a_blank_remote_path_means_no_start_path() {
    assert_eq!(entry_with_options(&[("remote_path", "   ")]).start_path(), None);
}

#[test]
fn a_profile_without_a_remote_path_has_no_start_path() {
    assert_eq!(entry_with_options(&[]).start_path(), None);
}

#[test]
fn profile_and_entry_debug_show_option_keys_but_never_their_values() {
    let profile = ConnectionProfile {
        name: "s3".to_string(),
        host: "h".to_string(),
        protocol: "s3".to_string(),
        port: None,
        username: "u".to_string(),
        options: BTreeMap::from([("secret_key".to_string(), "s3cr3t".to_string())]),
        group: None,
        tags: Vec::new(),
        in_keyring: Vec::new(),
    };
    let entry = ConnectionEntry::from_profile(profile.clone(), 443);

    for printed in [format!("{profile:?}"), format!("{entry:?}")] {
        assert!(printed.contains("secret_key"), "{printed}");
        assert!(!printed.contains("s3cr3t"), "{printed}");
    }
}

#[test]
fn an_old_profile_without_group_or_tags_loads_with_none_and_empty() {
    let profile: ConnectionProfile = toml::from_str("host = \"h\"\nusername = \"u\"\n").unwrap();
    assert_eq!(profile.group, None);
    assert!(profile.tags.is_empty());
    assert!(profile.options.is_empty());
}

#[test]
fn group_and_tags_are_not_options_and_are_omitted_when_empty() {
    let profile: ConnectionProfile =
        toml::from_str("host = \"h\"\nusername = \"u\"\ngroup = \"Work/Web\"\ntags = [\"prod\"]\n").unwrap();
    assert_eq!(profile.group.as_deref(), Some("Work/Web"));
    assert_eq!(profile.tags, vec!["prod"]);
    assert!(profile.options.is_empty());

    let bare = ConnectionProfile { group: None, tags: Vec::new(), ..profile };
    let written = toml::to_string(&bare).unwrap();
    assert!(!written.contains("group") && !written.contains("tags"), "{written}");
}

#[test]
fn from_profile_carries_group_and_tags() {
    let profile: ConnectionProfile =
        toml::from_str("host = \"h\"\nusername = \"u\"\ngroup = \"A\"\ntags = [\"t\"]\n").unwrap();
    let entry = ConnectionEntry::from_profile(profile, 22);
    assert_eq!((entry.group.as_deref(), entry.tags.clone()), (Some("A"), vec!["t".to_string()]));
}

#[test]
fn an_orphan_labels_entry_carries_its_labels_and_cannot_be_mistaken_for_a_host() {
    let entry = ConnectionEntry::orphan_labels(
        "web1".into(),
        Labels { group: Some("Work".into()), tags: vec!["prod".into()], in_keyring: Vec::new() },
        ConnectionSource::ShadowedSshHost,
    );
    assert_eq!(entry.source, ConnectionSource::ShadowedSshHost);
    assert_eq!((entry.host.as_str(), entry.port), ("", 0));
    assert_eq!(entry.group.as_deref(), Some("Work"));
    assert_eq!(entry.tags, vec!["prod"]);
}

#[test]
fn only_label_records_without_a_host_count_as_orphans() {
    assert!(ConnectionSource::MissingSshHost.is_orphan_labels());
    assert!(ConnectionSource::ShadowedSshHost.is_orphan_labels());
    assert!(!ConnectionSource::Profile.is_orphan_labels());
    assert!(!ConnectionSource::SshConfig.is_orphan_labels());
}

#[test]
fn an_old_password_key_is_dropped_and_not_an_option() {
    let profile: ConnectionProfile = toml::from_str("host = \"h\"\nusername = \"u\"\npassword = \"x\"\n").unwrap();
    assert!(profile.options.is_empty());
    let written = toml::to_string(&profile).unwrap();
    assert!(!written.contains("password") && !written.contains("\"x\""), "{written}");
}

#[test]
fn markers_round_trip_and_are_omitted_when_empty() {
    let profile: ConnectionProfile =
        toml::from_str("host = \"h\"\nusername = \"u\"\nin_keyring = [\"password\", \"token\"]\n").unwrap();
    assert_eq!(profile.in_keyring, vec!["password", "token"]);
    assert!(profile.options.is_empty());
    let written = toml::to_string(&profile).unwrap();
    assert!(written.contains("in_keyring"), "{written}");
    let bare = ConnectionProfile { in_keyring: Vec::new(), ..profile };
    assert!(!toml::to_string(&bare).unwrap().contains("in_keyring"));
}

#[test]
fn an_entry_knows_a_password_is_saved_without_holding_it() {
    let profile: ConnectionProfile =
        toml::from_str("host = \"h\"\nusername = \"u\"\nin_keyring = [\"password\"]\n").unwrap();
    let entry = ConnectionEntry::from_profile(profile, 22);
    assert!(entry.saved_password);
    assert_eq!(entry.password, None);
    assert_eq!(entry.in_keyring, vec!["password"]);
}

#[test]
fn a_saved_secret_option_alone_is_not_a_saved_password() {
    let profile: ConnectionProfile =
        toml::from_str("host = \"h\"\nusername = \"u\"\nin_keyring = [\"token\"]\n").unwrap();
    assert!(!ConnectionEntry::from_profile(profile, 22).saved_password);
}

#[test]
fn discovered_and_orphan_entries_start_without_markers() {
    let discovered = ConnectionEntry::discovered(
        "sftp",
        Target {
            name: "web1".into(),
            host: "h".into(),
            port: 22,
            username: "u".into(),
            password: None,
            options: BTreeMap::new(),
        },
    );
    assert!(!discovered.saved_password && discovered.in_keyring.is_empty());
    let orphan = ConnectionEntry::orphan_labels("x".into(), Labels::default(), ConnectionSource::MissingSshHost);
    assert!(!orphan.saved_password && orphan.in_keyring.is_empty());
}
