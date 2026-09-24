pub mod draft;
pub mod profile;
pub mod store;

use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
pub use draft::ProfileDraft;
pub use profile::{ConnectionEntry, ConnectionProfile, ConnectionSource, DEFAULT_PROTOCOL};
use termconnect_vfs::{Environment, Protocol};

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

    for protocol in protocols {
        for target in protocol.discover(env)? {
            if !known_names.insert(target.name.clone()) {
                continue;
            }

            entries.push(ConnectionEntry::discovered(protocol.id(), target));
        }
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());

    Ok(entries)
}

#[cfg(test)]
mod tests;
