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
