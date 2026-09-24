use std::sync::Arc;

use termconnect_vfs::{Environment, FileSystem, testing::FakeFs};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::{Engine, EngineParts, Event, Internal, LiveSession, SessionId};
use crate::{
    Paths, Severity,
    config::{bookmarks::Bookmarks, settings::TransferSettings},
    profiles::{ConnectionEntry, ConnectionSource},
};

pub(crate) struct TestEngine {
    pub(crate) engine: Engine,
    pub(crate) events: UnboundedReceiver<Event>,
    pub(crate) internal: UnboundedReceiver<Internal>,
    pub(crate) dir: tempfile::TempDir,
}

pub(crate) fn test_engine() -> TestEngine {
    let dir = tempfile::tempdir().unwrap();
    let (events_tx, events) = unbounded_channel();
    let (internal_tx, internal) = unbounded_channel();
    let parts = EngineParts {
        paths: Paths::in_dir(dir.path()),
        env: Environment::default(),
        protocols: Vec::new(),
        local_fs: Arc::new(termconnect_lfs::LocalFs::new(dir.path().to_path_buf())),
        transfers: TransferSettings::default(),
        bookmarks: Bookmarks::default(),
    };
    TestEngine { engine: Engine::new(parts, events_tx, internal_tx), events, internal, dir }
}

pub(crate) fn sample_entry(name: &str) -> ConnectionEntry {
    ConnectionEntry {
        name: name.to_string(),
        protocol: "fake".to_string(),
        host: format!("{name}.example.com"),
        port: 22,
        username: "user".to_string(),
        password: None,
        options: Default::default(),
        source: ConnectionSource::Profile,
    }
}

impl TestEngine {
    pub(crate) fn add_session(&mut self, name: &str) -> (SessionId, FakeFs) {
        let fs = FakeFs::new();
        let id = self.add_session_with(name, Arc::new(fs.clone()));
        (id, fs)
    }

    pub(crate) fn add_session_with(&mut self, name: &str, fs: Arc<dyn FileSystem>) -> SessionId {
        let id = self.engine.next_session_id;
        self.engine.next_session_id += 1;
        self.engine.sessions.insert(
            id,
            LiveSession {
                name: name.to_string(),
                entry: sample_entry(name),
                protocol: Arc::new(termconnect_vfs::testing::FakeProtocol::new(FakeFs::new())),
                fs,
            },
        );
        id
    }

    pub(crate) fn drain(&mut self) -> Vec<Event> {
        std::iter::from_fn(|| self.events.try_recv().ok()).collect()
    }

    pub(crate) fn notices(&mut self) -> Vec<(Severity, String)> {
        self.drain()
            .into_iter()
            .filter_map(|event| match event {
                Event::Notice { severity, message } => Some((severity, message)),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn first_notice(&mut self) -> Option<(Severity, String)> {
        self.notices().into_iter().next()
    }

    pub(crate) async fn next_event(&mut self) -> Event {
        tokio::time::timeout(std::time::Duration::from_secs(2), self.events.recv())
            .await
            .expect("an event within two seconds")
            .expect("the event channel is open")
    }

    pub(crate) async fn run_internal(&mut self) {
        let done = tokio::time::timeout(std::time::Duration::from_secs(2), self.internal.recv())
            .await
            .expect("an internal event within two seconds")
            .expect("the internal channel is open");
        self.engine.handle_internal(done);
    }
}
