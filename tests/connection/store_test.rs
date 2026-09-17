use super::*;

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
    assert_eq!(profiles[0].port, 2222);
    assert_eq!(profiles[0].username, "deploy");
    assert_eq!(
        profiles[0].identity_file,
        Some(PathBuf::from("/home/user/.ssh/id_ed25519"))
    );
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

    assert_eq!(profiles[0].port, 22);
}

use std::os::unix::fs::PermissionsExt;

#[test]
fn save_to_creates_a_new_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let profile = ConnectionProfile {
        name: "prod".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
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
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
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
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            remote_path: None,
            password: None,
        },
    )
    .unwrap();
    save_to(
        &path,
        &ConnectionProfile {
            name: "staging".to_string(),
            host: "b.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            remote_path: None,
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
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            remote_path: None,
            password: Some("hunter2".to_string()),
        },
    )
    .unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
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
            port: 22,
            username: "deploy".to_string(),
            identity_file: None,
            remote_path: None,
            password: None,
        },
    )
    .unwrap();

    assert!(path.exists());
}
