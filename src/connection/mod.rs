pub mod client;
pub mod profile;
pub mod session;
pub mod ssh_config;
pub mod store;

use anyhow::Result;

pub use profile::{ConnectionEntry, ConnectionSource};

/// The connections available to dial: saved profiles first, then any
/// `~/.ssh/config` hosts not already saved as a profile — so the same host
/// alias never appears twice, and a saved profile always wins.
pub fn list_all() -> Result<Vec<ConnectionEntry>> {
    let profiles = store::load()?;
    let known_names: std::collections::HashSet<String> = profiles
        .iter()
        .map(|profile| profile.name.clone())
        .collect();

    let mut entries: Vec<ConnectionEntry> =
        profiles.into_iter().map(ConnectionEntry::from).collect();

    for host in ssh_config::load()? {
        if known_names.contains(host.name.as_str()) {
            continue;
        }

        entries.push(ConnectionEntry {
            name: host.name,
            host: host.host_name.unwrap_or_default(),
            port: host.port.unwrap_or(22),
            username: host.user.unwrap_or_else(default_username),
            identity_file: host.identity_file,
            remote_path: None,
            password: None,
            source: ConnectionSource::SshConfig,
        });
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());

    Ok(entries)
}

fn default_username() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
}
