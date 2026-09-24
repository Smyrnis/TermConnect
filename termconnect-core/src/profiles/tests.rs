use std::sync::Arc;

use termconnect_vfs::{Environment, Protocol, testing::FakeFs};

use super::*;

struct Discovering(Vec<&'static str>);

#[termconnect_vfs::async_trait]
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

    fn discover(&self, _env: &Environment) -> Result<Vec<termconnect_vfs::Target>, termconnect_vfs::ProtocolError> {
        Ok(self
            .0
            .iter()
            .map(|name| termconnect_vfs::Target {
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
        &self, _target: &termconnect_vfs::Target, _prompter: &mut dyn termconnect_vfs::Prompter,
    ) -> Result<Arc<dyn termconnect_vfs::FileSystem>, termconnect_vfs::ProtocolError> {
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
