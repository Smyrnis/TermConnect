use std::sync::Arc;

use porthmos_vfs::{FileSystem, Protocol};

use super::super::{Engine, Event, Internal, LiveSession, Location, SessionId, prompter::EnginePrompter};
use crate::{Severity, connect_failure_message, profiles::ConnectionEntry};

impl Engine {
    pub(crate) fn connect(&mut self, profile: &str) {
        if let Some((&session, live)) = self.sessions.iter().find(|(_, live)| live.name == profile) {
            let shell_available = live.protocol.shell_command(&live.entry.target(), &self.env).is_some();
            self.emit(Event::Connected { session, name: live.name.clone(), shell_available });
            return;
        }

        let entry = match self.all_profiles() {
            Ok(entries) => entries.into_iter().find(|entry| entry.name == profile),
            Err(err) => {
                self.notice(Severity::Error, err.to_string());
                return;
            }
        };
        let Some(entry) = entry else {
            self.emit(Event::ConnectFailed {
                name: profile.to_string(),
                message: format!("No saved connection named \"{profile}\""),
            });
            return;
        };
        let Some(protocol) = self.protocols.iter().find(|protocol| protocol.id() == entry.protocol).cloned() else {
            self.emit(Event::ConnectFailed {
                name: entry.name.clone(),
                message: format!("No \"{}\" protocol is available for {}", entry.protocol, entry.name),
            });
            return;
        };

        self.emit(Event::Connecting { name: entry.name.clone() });
        let internal = self.internal.clone();
        let mut prompter = EnginePrompter { questions: self.questions.clone(), events: self.events.clone() };
        tokio::spawn(async move {
            let done = match protocol.connect(&entry.target(), &mut prompter).await {
                Ok(fs) => Internal::Connected { entry, protocol, fs },
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
    ) {
        let session = self.next_session_id;
        self.next_session_id += 1;
        let shell_available = protocol.shell_command(&entry.target(), &self.env).is_some();
        let name = entry.name.clone();
        self.sessions.insert(session, LiveSession { name: name.clone(), entry, protocol, fs });
        self.emit(Event::Connected { session, name, shell_available });
        self.list(Location::Session(session), None);
    }

    pub(crate) fn disconnect(&mut self, session: SessionId) {
        let Some(live) = self.sessions.remove(&session) else {
            return;
        };

        let affected = self.transfers.fail_queued_for_session(session, "session disconnected")
            + self.cancel_session_transfers(session)
            + self.drop_conflict_reviews(|review| review.session_id == session);
        if affected > 0 {
            let plural = if affected == 1 { "" } else { "s" };
            self.notice(Severity::Info, format!("{affected} transfer{plural} cancelled \u{2014} session disconnected"));
        }

        self.notice(Severity::Info, format!("Disconnected from {}", live.name));
        self.emit(Event::Disconnected { session, name: live.name });
    }

    pub(crate) fn prepare_shell(&mut self, session: SessionId) {
        let Some(live) = self.sessions.get(&session) else {
            self.notice(Severity::Warning, "Connect to a server first");
            return;
        };
        match live.protocol.shell_command(&live.entry.target(), &self.env) {
            Some(invocation) => self.emit(Event::ShellReady { session, invocation }),
            None => self.notice(Severity::Warning, "This connection doesn't support a terminal session"),
        }
    }
}

#[cfg(test)]
mod tests;
