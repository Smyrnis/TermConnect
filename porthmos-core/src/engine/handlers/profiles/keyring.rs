use std::{collections::BTreeMap, future::Future, pin::Pin};

use tokio::sync::mpsc::unbounded_channel;
use zeroize::Zeroizing;

use super::super::super::{Engine, Internal};
use crate::{
    Severity,
    profiles::{PASSWORD_MARKER, SecretEdit, store},
    secrets::{SecretField, SecretKey},
    tasks::Scope,
};

pub(crate) struct KeyringDone {
    pub(crate) job: u64,
    pub(crate) owner: SecretOwner,
    pub(crate) rollback: Vec<String>,
    pub(crate) restore: Vec<String>,
    pub(crate) errors: Vec<String>,
}

pub(crate) type KeyringJob = Pin<Box<dyn Future<Output = Internal> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SecretOwner {
    Profile(String),
    SshHost(String),
}

impl SecretOwner {
    fn key(&self) -> String {
        match self {
            SecretOwner::Profile(name) => format!("profile:{name}"),
            SecretOwner::SshHost(alias) => format!("ssh:{alias}"),
        }
    }

    fn name(&self) -> &str {
        match self {
            SecretOwner::Profile(name) | SecretOwner::SshHost(name) => name,
        }
    }
}

pub(crate) fn profile_account(name: &str, marker: &str) -> String {
    let field = if marker == PASSWORD_MARKER { SecretField::Password } else { SecretField::Option(marker.to_string()) };
    SecretKey::Profile { name: name.to_string(), field }.account()
}

pub(crate) fn ssh_account(alias: &str) -> String {
    SecretKey::SshHost { alias: alias.to_string() }.account()
}

pub(crate) fn save_failed(name: &str, err: impl std::fmt::Display) -> String {
    format!("Couldn't save the password for {name} in the system keyring: {err}")
}

fn remove_failed(name: &str, err: impl std::fmt::Display) -> String {
    format!("Couldn't remove the password for {name} from the system keyring: {err}")
}

pub(crate) struct SecretChanges {
    pub(crate) name: String,
    pub(crate) renamed_from: Option<String>,
    pub(crate) saved_before: Vec<String>,
    pub(crate) kept: Vec<String>,
    pub(crate) replaced: BTreeMap<String, Zeroizing<String>>,
}

impl SecretChanges {
    pub(crate) fn from_edits(
        name: String, renamed_from: Option<String>, saved_before: Vec<String>, kept: Vec<String>,
        edits: impl IntoIterator<Item = (String, SecretEdit)>,
    ) -> Self {
        let replaced = edits
            .into_iter()
            .filter_map(|(marker, edit)| match edit {
                SecretEdit::Replace(secret) => Some((marker, Zeroizing::new(secret))),
                SecretEdit::Keep | SecretEdit::Clear => None,
            })
            .collect();
        Self { name, renamed_from, saved_before, kept, replaced }
    }

    fn dropped(&self) -> Vec<String> {
        self.saved_before
            .iter()
            .filter(|marker| !self.kept.contains(marker) && !self.replaced.contains_key(*marker))
            .cloned()
            .collect()
    }
}

impl Engine {
    fn enqueue_keyring_job(&mut self, owner: &SecretOwner, job: impl FnOnce(u64) -> KeyringJob) {
        self.next_keyring_job += 1;
        let id = self.next_keyring_job;
        self.latest_keyring_job.insert(owner.key(), id);
        let internal = self.internal.clone();
        let tasks = self.tasks.clone();
        let queue = self.keyring_jobs.get_or_insert_with(|| {
            let (queue, mut jobs) = unbounded_channel::<KeyringJob>();
            tasks.spawn("keyring-queue", Scope::Background, move |_| async move {
                while let Some(job) = jobs.recv().await {
                    let _ = internal.send(job.await);
                }
            });
            queue
        });
        let _ = queue.send(job(id));
    }

    fn move_profile_cache(&self, old: &str, new: &str) {
        for account in self.secrets.cached_accounts() {
            if let Some(SecretKey::Profile { name, field }) = SecretKey::parse(&account)
                && name == old
            {
                self.secrets.move_cached(&account, &SecretKey::Profile { name: new.to_string(), field }.account());
            }
        }
    }

    fn uncache_profile(&self, profile: &str) {
        for account in self.secrets.cached_accounts() {
            if matches!(SecretKey::parse(&account), Some(SecretKey::Profile { name, .. }) if name == profile) {
                self.secrets.uncache(&account);
            }
        }
    }

    fn profile_markers(&self, name: &str) -> Vec<String> {
        store::load(&self.paths)
            .ok()
            .and_then(|profiles| profiles.into_iter().find(|profile| profile.name == name))
            .map(|profile| profile.in_keyring)
            .unwrap_or_default()
    }

    pub(crate) fn ssh_markers(&self, alias: &str) -> Vec<String> {
        store::load_ssh_labels(&self.paths)
            .ok()
            .and_then(|records| records.get(alias).map(|record| record.in_keyring.clone()))
            .unwrap_or_default()
    }

    fn write_markers(&mut self, owner: &SecretOwner, markers: &[String]) {
        let written = match owner {
            SecretOwner::Profile(name) => store::set_profile_markers(&self.paths, name, markers).map(|_| ()),
            SecretOwner::SshHost(alias) => store::set_ssh_markers(&self.paths, alias, markers),
        };
        if let Err(err) = written {
            self.report(Severity::Error, err.to_string());
        }
    }

    fn add_marker(&mut self, owner: &SecretOwner, marker: &str) {
        let mut markers = match owner {
            SecretOwner::Profile(name) => self.profile_markers(name),
            SecretOwner::SshHost(alias) => self.ssh_markers(alias),
        };
        if !markers.iter().any(|saved| saved == marker) {
            markers.push(marker.to_string());
            self.write_markers(owner, &markers);
        }
    }

    pub(crate) fn apply_secret_changes(&mut self, changes: SecretChanges) {
        let owner = SecretOwner::Profile(changes.name.clone());
        if let Some(old) = &changes.renamed_from {
            self.move_profile_cache(old, &changes.name);
        }
        let dropped = changes.dropped();
        for marker in &dropped {
            self.secrets.uncache(&profile_account(&changes.name, marker));
        }
        for (marker, secret) in &changes.replaced {
            self.secrets.remember(&profile_account(&changes.name, marker), secret);
        }
        let moves = changes.renamed_from.is_some() && !changes.saved_before.is_empty();
        let keyring = self.secrets.available() && (moves || !changes.replaced.is_empty() || !dropped.is_empty());
        if !keyring {
            return;
        }
        let mut markers = changes.kept.clone();
        markers.extend(changes.replaced.keys().filter(|marker| !changes.kept.contains(marker)).cloned());
        self.write_markers(&owner, &markers);
        let secrets = self.secrets.clone();
        self.enqueue_keyring_job(&owner, move |job| {
            Box::pin(async move {
                let name = changes.name;
                let mut rollback = Vec::new();
                let mut errors = Vec::new();
                if let Some(old) = &changes.renamed_from {
                    for marker in &changes.saved_before {
                        let moved =
                            secrets.move_keyring(&profile_account(old, marker), &profile_account(&name, marker)).await;
                        if let Err(err) = moved {
                            errors.push(save_failed(&name, err));
                            rollback.push(marker.clone());
                        }
                    }
                }
                for (marker, secret) in &changes.replaced {
                    match secrets.write_keyring(&profile_account(&name, marker), secret).await {
                        Ok(true) => {}
                        Ok(false) => rollback.push(marker.clone()),
                        Err(err) => {
                            errors.push(save_failed(&name, err));
                            rollback.push(marker.clone());
                        }
                    }
                }
                for marker in dropped {
                    if let Err(err) = secrets.erase_keyring(&profile_account(&name, &marker)).await {
                        errors.push(remove_failed(&name, err));
                    }
                }
                Internal::KeyringDone(KeyringDone {
                    job,
                    owner: SecretOwner::Profile(name),
                    rollback,
                    restore: Vec::new(),
                    errors,
                })
            })
        });
    }

    pub(crate) fn forget_profile_secrets(&mut self, name: String, saved: Vec<String>) {
        self.uncache_profile(&name);
        if saved.is_empty() || !self.secrets.available() {
            return;
        }
        let owner = SecretOwner::Profile(name.clone());
        let secrets = self.secrets.clone();
        self.enqueue_keyring_job(&owner, move |job| {
            Box::pin(async move {
                let mut errors = Vec::new();
                for marker in &saved {
                    if let Err(err) = secrets.erase_keyring(&profile_account(&name, marker)).await {
                        errors.push(remove_failed(&name, err));
                    }
                }
                Internal::KeyringDone(KeyringDone {
                    job,
                    owner: SecretOwner::Profile(name),
                    rollback: Vec::new(),
                    restore: Vec::new(),
                    errors,
                })
            })
        });
    }

    pub(crate) fn save_typed_password(&mut self, owner: SecretOwner, account: String, secret: Zeroizing<String>) {
        if !self.secrets.available() {
            return;
        }
        self.add_marker(&owner, PASSWORD_MARKER);
        let secrets = self.secrets.clone();
        self.enqueue_keyring_job(&owner.clone(), move |job| {
            Box::pin(async move {
                let (rollback, errors) = match secrets.write_keyring(&account, &secret).await {
                    Ok(true) => (Vec::new(), Vec::new()),
                    Ok(false) => (vec![PASSWORD_MARKER.to_string()], Vec::new()),
                    Err(err) => (vec![PASSWORD_MARKER.to_string()], vec![save_failed(owner.name(), err)]),
                };
                Internal::KeyringDone(KeyringDone { job, owner, rollback, restore: Vec::new(), errors })
            })
        });
    }

    pub(crate) fn move_ssh_password(&mut self, from: &str, to: &str, saved: Vec<String>) {
        let (from_account, to_account) = (ssh_account(from), ssh_account(to));
        self.secrets.move_cached(&from_account, &to_account);
        if saved.is_empty() || !self.secrets.available() {
            return;
        }
        let owner = SecretOwner::SshHost(to.to_string());
        let secrets = self.secrets.clone();
        self.enqueue_keyring_job(&owner.clone(), move |job| {
            Box::pin(async move {
                let (rollback, errors) = match secrets.move_keyring(&from_account, &to_account).await {
                    Ok(()) => (Vec::new(), Vec::new()),
                    Err(err) => (saved, vec![save_failed(owner.name(), err)]),
                };
                Internal::KeyringDone(KeyringDone { job, owner, rollback, restore: Vec::new(), errors })
            })
        });
    }

    pub(crate) fn drop_ssh_password(&mut self, alias: &str, saved: Vec<String>, keep_record: bool) {
        let account = ssh_account(alias);
        self.secrets.uncache(&account);
        if saved.is_empty() || !self.secrets.available() {
            return;
        }
        let owner = SecretOwner::SshHost(alias.to_string());
        if keep_record {
            self.write_markers(&owner, &[]);
        }
        let secrets = self.secrets.clone();
        self.enqueue_keyring_job(&owner.clone(), move |job| {
            Box::pin(async move {
                let errors = match secrets.erase_keyring(&account).await {
                    Ok(()) => Vec::new(),
                    Err(err) => vec![remove_failed(owner.name(), err)],
                };
                let restore = if keep_record && !errors.is_empty() { saved } else { Vec::new() };
                Internal::KeyringDone(KeyringDone { job, owner, rollback: Vec::new(), restore, errors })
            })
        });
    }

    pub(crate) fn finish_keyring_job(&mut self, done: KeyringDone) {
        let KeyringDone { job, owner, rollback, restore, errors } = done;
        let latest = self.latest_keyring_job.get(&owner.key()) == Some(&job);
        if latest {
            self.latest_keyring_job.remove(&owner.key());
        }
        for error in errors {
            self.report(Severity::Error, error);
        }
        if latest && (!rollback.is_empty() || !restore.is_empty()) {
            let current = match &owner {
                SecretOwner::Profile(name) => self.profile_markers(name),
                SecretOwner::SshHost(alias) => self.ssh_markers(alias),
            };
            let mut markers: Vec<String> = current.into_iter().filter(|marker| !rollback.contains(marker)).collect();
            markers.extend(restore.into_iter().filter(|marker| !markers.contains(marker)).collect::<Vec<_>>());
            let exists = match &owner {
                SecretOwner::Profile(name) => store::load(&self.paths)
                    .ok()
                    .is_some_and(|profiles| profiles.iter().any(|profile| &profile.name == name)),
                SecretOwner::SshHost(_) => true,
            };
            if exists {
                self.write_markers(&owner, &markers);
            }
        }
        self.list_profiles();
    }
}
