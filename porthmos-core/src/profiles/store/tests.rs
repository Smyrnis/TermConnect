use super::*;
use crate::profiles::Labels;

#[test]
fn config_path_is_connections_toml_under_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.config_dir.join("connections.toml"), "[connections.web]\nhost = \"h\"\nusername = \"u\"\n")
        .unwrap();

    let names: Vec<String> = load(&paths).unwrap().into_iter().map(|profile| profile.name).collect();

    assert_eq!(names, vec!["web".to_string()]);
}

#[test]
fn load_from_a_missing_file_returns_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let profiles = load_from(&path).unwrap();

    assert!(profiles.is_empty());
}

#[test]
fn load_from_parses_connection_tables_and_fills_in_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"
[connections.production]
host = "server.example.com"
port = 2222
username = "deploy"
identity_file = "/home/user/.ssh/id_ed25519"
remote_path = "/var/www/app"
"#,
    )
    .unwrap();

    let profiles = load_from(&path).unwrap();

    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "production");
    assert_eq!(profiles[0].host, "server.example.com");
    assert_eq!(profiles[0].port, Some(2222));
    assert_eq!(profiles[0].username, "deploy");
    assert_eq!(profiles[0].options.get("identity_file").map(String::as_str), Some("/home/user/.ssh/id_ed25519"));
}

#[test]
fn load_from_defaults_port_to_22_when_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"
[connections.staging]
host = "staging.example.com"
username = "deploy"
"#,
    )
    .unwrap();

    let profiles = load_from(&path).unwrap();

    assert_eq!(profiles[0].port, None);
    assert_eq!(crate::profiles::ConnectionEntry::from_profile(profiles[0].clone(), 22).port, 22);
}

use std::os::unix::fs::PermissionsExt;

#[test]
fn save_to_creates_a_new_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: Some(22),
        username: "deploy".to_string(),
        options: std::collections::BTreeMap::new(),
        password: Some("hunter2".to_string()),
        group: None,
        tags: Vec::new(),
    };

    save_to(&path, &profile).unwrap();
    let loaded = load_from(&path).unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "prod");
    assert_eq!(loaded[0].password, Some("hunter2".to_string()));
}

#[test]
fn save_to_overwrites_an_existing_entry_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "old-host.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: Some(22),
        username: "deploy".to_string(),
        options: std::collections::BTreeMap::new(),
        password: None,
        group: None,
        tags: Vec::new(),
    };
    save_to(&path, &profile).unwrap();

    profile.host = "new-host.example.com".to_string();
    save_to(&path, &profile).unwrap();

    let loaded = load_from(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].host, "new-host.example.com");
}

#[test]
fn delete_from_removes_one_entry_and_leaves_others() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    save_to(
        &path,
        &ConnectionProfile {
            name: "prod".to_string(),
            host: "a.example.com".to_string(),
            protocol: "sftp".to_string(),
            port: Some(22),
            username: "deploy".to_string(),
            options: std::collections::BTreeMap::new(),
            password: None,
            group: None,
            tags: Vec::new(),
        },
    )
    .unwrap();
    save_to(
        &path,
        &ConnectionProfile {
            name: "staging".to_string(),
            host: "b.example.com".to_string(),
            protocol: "sftp".to_string(),
            port: Some(22),
            username: "deploy".to_string(),
            options: std::collections::BTreeMap::new(),
            password: None,
            group: None,
            tags: Vec::new(),
        },
    )
    .unwrap();

    delete_from(&path, "prod").unwrap();

    let loaded = load_from(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "staging");
}

#[test]
fn save_to_sets_file_permissions_to_owner_read_write_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    save_to(
        &path,
        &ConnectionProfile {
            name: "prod".to_string(),
            host: "a.example.com".to_string(),
            protocol: "sftp".to_string(),
            port: Some(22),
            username: "deploy".to_string(),
            options: std::collections::BTreeMap::new(),
            password: Some("hunter2".to_string()),
            group: None,
            tags: Vec::new(),
        },
    )
    .unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn save_to_leaves_the_original_file_untouched_if_the_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = ConnectionProfile {
        name: "prod".to_string(),
        host: "a.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: Some(22),
        username: "deploy".to_string(),
        options: std::collections::BTreeMap::new(),
        password: None,
        group: None,
        tags: Vec::new(),
    };
    save_to(&path, &original).unwrap();

    let mut perms = fs::metadata(dir.path()).unwrap().permissions();
    perms.set_mode(0o500);
    fs::set_permissions(dir.path(), perms.clone()).unwrap();

    let mut updated = original.clone();
    updated.host = "b.example.com".to_string();
    let result = save_to(&path, &updated);

    perms.set_mode(0o700);
    fs::set_permissions(dir.path(), perms).unwrap();

    assert!(result.is_err());
    let loaded = load_from(&path).unwrap();
    assert_eq!(loaded[0].host, "a.example.com");
}

#[test]
fn save_to_creates_the_parent_directory_if_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("config.toml");
    save_to(
        &path,
        &ConnectionProfile {
            name: "prod".to_string(),
            host: "a.example.com".to_string(),
            protocol: "sftp".to_string(),
            port: Some(22),
            username: "deploy".to_string(),
            options: std::collections::BTreeMap::new(),
            password: None,
            group: None,
            tags: Vec::new(),
        },
    )
    .unwrap();

    assert!(path.exists());
}

#[test]
fn an_existing_flat_profile_round_trips_without_new_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.toml");
    std::fs::write(
        &path,
        "[connections.web]\nhost = \"example.com\"\nport = 2222\nusername = \"deploy\"\nidentity_file = \"/home/u/.ssh/id_ed25519\"\nremote_path = \"/var/www\"\n",
    )
    .unwrap();
    let profiles = load_from(&path).unwrap();
    assert_eq!(profiles[0].protocol, "sftp");
    assert_eq!(profiles[0].options.get("remote_path").map(String::as_str), Some("/var/www"));

    let copy = dir.path().join("copy.toml");
    save_to(&copy, &profiles[0]).unwrap();
    let before: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let after: toml::Table = std::fs::read_to_string(&copy).unwrap().parse().unwrap();
    assert_eq!(before, after);
}

fn labels(group: &str, tags: &[&str]) -> Labels {
    Labels { group: Some(group.to_string()), tags: tags.iter().map(|tag| tag.to_string()).collect() }
}

fn sample_profile(name: &str) -> ConnectionProfile {
    ConnectionProfile {
        name: name.to_string(),
        protocol: "sftp".to_string(),
        host: "h".to_string(),
        port: None,
        username: "u".to_string(),
        password: None,
        group: None,
        tags: Vec::new(),
        options: std::collections::BTreeMap::new(),
    }
}

#[test]
fn ssh_labels_are_saved_loaded_and_kept_next_to_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.connections_file(), "[connections.web]\nhost = \"h\"\nusername = \"u\"\n").unwrap();

    save_ssh_labels(&paths, "web1", &labels("Work", &["prod"])).unwrap();

    assert_eq!(load_ssh_labels(&paths).unwrap().get("web1"), Some(&labels("Work", &["prod"])));
    assert_eq!(load(&paths).unwrap().len(), 1);
    let text = std::fs::read_to_string(paths.connections_file()).unwrap();
    assert!(text.contains("[ssh_hosts.web1]"), "{text}");
}

#[test]
fn saving_empty_labels_removes_the_record() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    save_ssh_labels(&paths, "web1", &labels("Work", &[])).unwrap();

    save_ssh_labels(&paths, "web1", &Labels::default()).unwrap();

    assert!(load_ssh_labels(&paths).unwrap().is_empty());
    let text = std::fs::read_to_string(paths.connections_file()).unwrap();
    assert!(!text.contains("ssh_hosts"), "{text}");
}

#[test]
fn moving_labels_replaces_any_record_at_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    save_ssh_labels(&paths, "old", &labels("A", &["x"])).unwrap();
    save_ssh_labels(&paths, "new", &labels("B", &[])).unwrap();

    move_ssh_labels(&paths, "old", "new").unwrap();

    let all = load_ssh_labels(&paths).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all.get("new"), Some(&labels("A", &["x"])));
}

#[test]
fn forgetting_labels_removes_only_that_record() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    save_ssh_labels(&paths, "a", &labels("A", &[])).unwrap();
    save_ssh_labels(&paths, "b", &labels("B", &[])).unwrap();

    forget_ssh_labels(&paths, "a").unwrap();

    assert_eq!(load_ssh_labels(&paths).unwrap().keys().collect::<Vec<_>>(), vec!["b"]);
}

#[test]
fn saving_a_profile_keeps_ssh_labels() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    save_ssh_labels(&paths, "web1", &labels("Work", &[])).unwrap();

    save(&paths, &sample_profile("web")).unwrap();

    assert_eq!(load_ssh_labels(&paths).unwrap().len(), 1);
}

#[test]
fn a_profile_with_group_and_tags_round_trips_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    let mut profile = sample_profile("web");
    profile.group = Some("Work/Web".into());
    profile.tags = vec!["prod".into(), "db".into()];

    save(&paths, &profile).unwrap();

    assert_eq!(load(&paths).unwrap(), vec![profile]);
}
