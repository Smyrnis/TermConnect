use std::sync::Arc;

use porthmos_vfs::{FileSystem, OptionKind, Protocol, Target};

use super::{
    super::{
        Engine, Event, Internal, LiveSession, SessionId,
        prompter::{EnginePrompter, TypedPassword},
    },
    profiles::{SecretOwner, profile_account, ssh_account},
};
use crate::{
    Severity, connect_failure_message,
    profiles::{ConnectionEntry, ConnectionSource, PASSWORD_MARKER},
    secrets::Secrets,
    tasks::Scope,
};

pub(crate) fn secret_account(entry: &ConnectionEntry) -> String {
    match entry.source {
        ConnectionSource::SshConfig => ssh_account(&entry.name),
        _ => profile_account(&entry.name, PASSWORD_MARKER),
    }
}

fn secret_option_keys(protocol: &dyn Protocol, entry: &ConnectionEntry) -> Vec<&'static str> {
    if entry.source == ConnectionSource::SshConfig {
        return Vec::new();
    }
    protocol
        .connection_form()
        .options
        .iter()
        .filter(|field| field.kind == OptionKind::Secret)
        .map(|field| field.key)
        .collect()
}

fn bare_target(entry: &ConnectionEntry, secret_keys: &[&str]) -> Target {
    let mut target = entry.target();
    target.password = None;
    target.options.retain(|key, _| !secret_keys.contains(&key.as_str()));
    target
}

async fn secured_target(entry: &ConnectionEntry, secret_keys: &[&'static str], secrets: &Secrets) -> Target {
    let mut target = bare_target(entry, secret_keys);
    target.password =
        secrets.lookup(&secret_account(entry), entry.saved_password).await.map(|secret| secret.to_string());
    for key in secret_keys {
        let saved = entry.in_keyring.iter().any(|marker| marker == key);
        if let Some(secret) = secrets.lookup(&profile_account(&entry.name, key), saved).await {
            target.options.insert(key.to_string(), secret.to_string());
        }
    }
    target
}

impl Engine {
    pub(crate) fn connect(&mut self, profile: &str) {
        if let Some((&session, live)) = self.sessions.iter().find(|(_, live)| live.name == profile) {
            let name = live.name.clone();
            let shell_available = self
                .shell_target(session)
                .is_some_and(|target| self.sessions[&session].protocol.shell_command(&target, &self.env).is_some());
            self.emit(Event::Connected { session, name, shell_available });
            return;
        }
        if self.connecting.contains(profile) {
            return;
        }

        let entries = match self.all_profiles() {
            Ok(entries) => entries,
            Err(err) => {
                self.report(Severity::Error, err.to_string());
                return;
            }
        };
        let named = |entry: &&ConnectionEntry| entry.name == profile;
        let entry = entries.iter().filter(named).find(|entry| !entry.source.is_orphan_labels()).cloned();
        let Some(entry) = entry else {
            if entries.iter().filter(named).any(|entry| entry.source == ConnectionSource::MissingSshHost) {
                self.connect_failed(profile.to_string(), format!("{profile} is no longer in ~/.ssh/config"));
                return;
            }
            self.connect_failed(profile.to_string(), format!("No saved connection named \"{profile}\""));
            return;
        };
        let Some(protocol) = self.protocols.iter().find(|protocol| protocol.id() == entry.protocol).cloned() else {
            self.connect_failed(
                entry.name.clone(),
                format!("No \"{}\" protocol is available for {}", entry.protocol, entry.name),
            );
            return;
        };

        self.emit(Event::Connecting { name: entry.name.clone() });
        self.connecting.insert(entry.name.clone());
        let secret_keys = secret_option_keys(protocol.as_ref(), &entry);
        let internal = self.internal.clone();
        let mut prompter =
            EnginePrompter { questions: self.questions.clone(), events: self.events.clone(), typed: None };
        let secrets = self.secrets.clone();
        self.tasks.spawn("connect", Scope::Background, move |_| async move {
            let target = secured_target(&entry, &secret_keys, &secrets).await;
            let done = match protocol.connect(&target, &mut prompter).await {
                Ok(fs) => Internal::Connected { entry, protocol, fs, typed: prompter.typed.take() },
                Err(err) => {
                    tracing::debug!("{err:?}");
                    let message = connect_failure_message(&err, &entry.name, protocol.display_name());
                    Internal::ConnectFailed { name: entry.name, message }
                }
            };
            let _ = internal.send(done);
        });
    }

    pub(crate) fn finish_connect(
        &mut self, entry: ConnectionEntry, protocol: Arc<dyn Protocol>, fs: Arc<dyn FileSystem>,
        typed: Option<TypedPassword>,
    ) {
        self.connecting.remove(&entry.name);
        if let Some(typed) = typed {
            self.keep_typed_password(&entry, typed);
        }
        let session = self.next_session_id;
        self.next_session_id += 1;
        let name = entry.name.clone();
        self.sessions.insert(session, LiveSession { name: name.clone(), entry, protocol, fs });
        let shell_available = self
            .shell_target(session)
            .is_some_and(|target| self.sessions[&session].protocol.shell_command(&target, &self.env).is_some());
        self.emit(Event::Connected { session, name, shell_available });
        self.open_start_directory(session);
    }

    fn keep_typed_password(&mut self, entry: &ConnectionEntry, typed: TypedPassword) {
        let account = secret_account(entry);
        self.secrets.remember(&account, &typed.secret);
        if !typed.save {
            return;
        }
        let owner = if entry.source == ConnectionSource::SshConfig {
            SecretOwner::SshHost(entry.name.clone())
        } else {
            SecretOwner::Profile(entry.name.clone())
        };
        self.save_typed_password(owner, account, typed.secret);
    }

    pub(crate) fn shell_target(&self, session: SessionId) -> Option<Target> {
        let live = self.sessions.get(&session)?;
        let secret_keys = secret_option_keys(live.protocol.as_ref(), &live.entry);
        let mut target = bare_target(&live.entry, &secret_keys);
        target.password = self.secrets.cached(&secret_account(&live.entry)).map(|secret| secret.to_string());
        for key in secret_keys {
            if let Some(secret) = self.secrets.cached(&profile_account(&live.entry.name, key)) {
                target.options.insert(key.to_string(), secret.to_string());
            }
        }
        Some(target)
    }

    pub(crate) fn disconnect(&mut self, session: SessionId) {
        let Some(live) = self.sessions.remove(&session) else {
            return;
        };
        self.history.remember_session(session, live.name.clone());

        let affected = self.transfers.fail_queued_for_session(session, "session disconnected")
            + self.cancel_session_transfers(session)
            + self.drop_conflict_reviews(|review| review.session_id == session);
        if affected > 0 {
            let plural = if affected == 1 { "" } else { "s" };
            self.info(format!("{affected} transfer{plural} cancelled \u{2014} session disconnected"));
        }

        self.info(format!("Disconnected from {}", live.name));
        self.emit(Event::Disconnected { session, name: live.name });
    }

    pub(crate) fn prepare_shell(&mut self, session: SessionId) {
        let Some(live) = self.sessions.get(&session) else {
            self.report(Severity::Warning, "Connect to a server first");
            return;
        };
        let Some(target) = self.shell_target(session) else {
            return;
        };
        match live.protocol.shell_command(&target, &self.env) {
            Some(invocation) => self.emit(Event::ShellReady { session, invocation }),
            None => self.report(Severity::Warning, "This connection doesn't support a terminal session"),
        }
    }
}

#[cfg(test)]
mod tests;
