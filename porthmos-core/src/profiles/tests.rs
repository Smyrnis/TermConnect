use std::sync::Arc;

use porthmos_vfs::{Environment, Protocol, testing::FakeFs};

use super::*;

struct Discovering(Vec<&'static str>);

#[porthmos_vfs::async_trait]
impl Protocol for Discovering {
    fn id(&self) -> &'static str {
        "discovering"
    }

    fn display_name(&self) -> &'static str {
        "DISCOVERING"
    }

    fn default_port(&self) -> u16 {
        22
    }

    fn discover(&self, _env: &Environment) -> Result<Vec<porthmos_vfs::Target>, porthmos_vfs::ProtocolError> {
        Ok(self
            .0
            .iter()
            .map(|name| porthmos_vfs::Target {
                name: name.to_string(),
                host: format!("{name}.example"),
                port: 22,
                username: "u".into(),
                password: None,
                options: Default::default(),
            })
            .collect())
    }

    async fn connect(
        &self, _target: &porthmos_vfs::Target, _prompter: &mut dyn porthmos_vfs::Prompter,
    ) -> Result<Arc<dyn porthmos_vfs::FileSystem>, porthmos_vfs::ProtocolError> {
        Ok(Arc::new(FakeFs::new()))
    }
}

#[test]
fn saved_profiles_shadow_discovered_hosts_with_the_same_name_and_the_list_is_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.connections_file(), "[connections.beta]\nhost = \"saved\"\nusername = \"me\"\n").unwrap();
    let protocols: Vec<Arc<dyn Protocol>> = vec![Arc::new(Discovering(vec!["gamma", "beta", "Alpha"]))];

    let entries = list_all(&paths, &protocols, &Environment::default()).unwrap();

    let summary: Vec<(&str, &str, ConnectionSource)> =
        entries.iter().map(|entry| (entry.name.as_str(), entry.host.as_str(), entry.source)).collect();
    assert_eq!(
        summary,
        vec![
            ("Alpha", "Alpha.example", ConnectionSource::SshConfig),
            ("beta", "saved", ConnectionSource::Profile),
            ("gamma", "gamma.example", ConnectionSource::SshConfig),
        ]
    );
}

fn paths_with(contents: &str) -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(dir.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(paths.connections_file(), contents).unwrap();
    (dir, paths)
}

#[test]
fn ssh_labels_apply_to_the_discovered_host() {
    let (_dir, paths) = paths_with("[ssh_hosts.web1]\ngroup = \"Work\"\ntags = [\"prod\"]\n");
    let protocols: Vec<Arc<dyn Protocol>> = vec![Arc::new(Discovering(vec!["web1"]))];

    let entries = list_all(&paths, &protocols, &Environment::default()).unwrap();

    assert_eq!(entries[0].source, ConnectionSource::SshConfig);
    assert_eq!(entries[0].group.as_deref(), Some("Work"));
    assert_eq!(entries[0].tags, vec!["prod"]);
}

#[test]
fn labels_for_a_host_that_is_gone_become_a_missing_entry() {
    let (_dir, paths) = paths_with("[ssh_hosts.old]\ngroup = \"Work\"\n");
    let protocols: Vec<Arc<dyn Protocol>> = vec![Arc::new(Discovering(vec!["new"]))];

    let entries = list_all(&paths, &protocols, &Environment::default()).unwrap();

    let summary: Vec<(&str, ConnectionSource)> =
        entries.iter().map(|entry| (entry.name.as_str(), entry.source)).collect();
    assert_eq!(summary, vec![("new", ConnectionSource::SshConfig), ("old", ConnectionSource::MissingSshHost)]);
    assert_eq!(entries[1].group.as_deref(), Some("Work"));
}

#[test]
fn labels_for_a_host_shadowed_by_a_saved_profile_become_a_shadowed_entry() {
    let (_dir, paths) =
        paths_with("[connections.web1]\nhost = \"h\"\nusername = \"u\"\n[ssh_hosts.web1]\ngroup = \"Work\"\n");
    let protocols: Vec<Arc<dyn Protocol>> = vec![Arc::new(Discovering(vec!["web1"]))];

    let entries = list_all(&paths, &protocols, &Environment::default()).unwrap();

    let sources: Vec<ConnectionSource> = entries.iter().map(|entry| entry.source).collect();
    assert_eq!(sources, vec![ConnectionSource::Profile, ConnectionSource::ShadowedSshHost]);
}

#[test]
fn labels_named_like_a_profile_but_gone_from_the_ssh_config_are_missing() {
    let (_dir, paths) =
        paths_with("[connections.web1]\nhost = \"h\"\nusername = \"u\"\n[ssh_hosts.web1]\ngroup = \"Work\"\n");
    let protocols: Vec<Arc<dyn Protocol>> = vec![Arc::new(Discovering(Vec::new()))];

    let entries = list_all(&paths, &protocols, &Environment::default()).unwrap();

    let sources: Vec<ConnectionSource> = entries.iter().map(|entry| entry.source).collect();
    assert_eq!(sources, vec![ConnectionSource::Profile, ConnectionSource::MissingSshHost]);
}

#[test]
fn hand_written_groups_and_tags_are_normalized_when_listed() {
    let (_dir, paths) = paths_with(
        "[connections.a]\nhost = \"h\"\nusername = \"u\"\ngroup = \"Prod/\"\ntags = [\" x\", \"X\"]\n\
         [connections.b]\nhost = \"h\"\nusername = \"u\"\ngroup = \"\"\n\
         [ssh_hosts.web1]\ngroup = \"a//b\"\n",
    );
    let protocols: Vec<Arc<dyn Protocol>> = vec![Arc::new(Discovering(vec!["web1"]))];

    let entries = list_all(&paths, &protocols, &Environment::default()).unwrap();

    let groups: Vec<Option<&str>> = entries.iter().map(|entry| entry.group.as_deref()).collect();
    assert_eq!(groups, vec![Some("Prod"), None, Some("a/b")]);
    assert_eq!(entries[0].tags, vec!["x"]);
}
