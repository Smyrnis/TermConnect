pub mod draft;
pub mod labels;
pub mod profile;
pub mod store;

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
pub use draft::ProfileDraft;
pub use labels::Labels;
use porthmos_vfs::{Environment, Protocol};
pub use profile::{ConnectionEntry, ConnectionProfile, ConnectionSource, DEFAULT_PROTOCOL};

use crate::Paths;

const FALLBACK_PORT: u16 = 22;

pub fn default_port_for(protocols: &[Arc<dyn Protocol>], protocol_id: &str) -> u16 {
    protocols
        .iter()
        .find(|protocol| protocol.id() == protocol_id)
        .map_or(FALLBACK_PORT, |protocol| protocol.default_port())
}

pub fn list_all(paths: &Paths, protocols: &[Arc<dyn Protocol>], env: &Environment) -> Result<Vec<ConnectionEntry>> {
    let profiles = store::load(paths)?;
    let mut known_names: HashSet<String> = profiles.iter().map(|profile| profile.name.clone()).collect();

    let mut entries: Vec<ConnectionEntry> = profiles
        .into_iter()
        .map(|profile| {
            let default_port = default_port_for(protocols, &profile.protocol);
            ConnectionEntry::from_profile(profile, default_port)
        })
        .collect();

    let mut shadowed_names = HashSet::new();
    for protocol in protocols {
        for target in protocol.discover(env)? {
            if !known_names.insert(target.name.clone()) {
                shadowed_names.insert(target.name);
                continue;
            }

            entries.push(ConnectionEntry::discovered(protocol.id(), target));
        }
    }

    let mut labels = store::load_ssh_labels(paths)?;
    for entry in entries.iter_mut().filter(|entry| entry.source == ConnectionSource::SshConfig) {
        if let Some(found) = labels.remove(&entry.name) {
            entry.group = found.group;
            entry.tags = found.tags;
        }
    }
    entries.extend(labels.into_iter().map(|(name, found)| {
        let source = if shadowed_names.contains(&name) {
            ConnectionSource::ShadowedSshHost
        } else {
            ConnectionSource::MissingSshHost
        };
        ConnectionEntry::orphan_labels(name, found, source)
    }));

    for entry in &mut entries {
        entry.group = entry.group.as_deref().and_then(labels::normalize_group);
        entry.tags = labels::normalize_tags(&entry.tags);
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());

    Ok(entries)
}

#[cfg(test)]
mod tests;
