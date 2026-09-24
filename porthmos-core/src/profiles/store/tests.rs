use super::*;

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
