use std::sync::{Arc, Mutex};

use porthmos_vfs::{Environment, FileSystem, testing::FakeFs};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::{Engine, EngineParts, Event, Internal, LiveSession, SessionId};
use crate::{
    Paths, Severity,
    config::{bookmarks::Bookmarks, settings::TransferSettings},
    profiles::{ConnectionEntry, ConnectionSource},
    secrets::{SecretBackend, Secrets, TestBackend},
};

pub(crate) struct TestEngine {
    pub(crate) engine: Engine,
    pub(crate) events: UnboundedReceiver<Event>,
    pub(crate) internal: UnboundedReceiver<Internal>,
    pub(crate) dir: tempfile::TempDir,
}

pub(crate) fn test_engine() -> TestEngine {
    engine_with(Secrets::new(None))
}

pub(crate) fn test_engine_with_secrets(backend: Arc<TestBackend>) -> TestEngine {
    let backend: Arc<dyn SecretBackend> = backend;
    engine_with(
        Secrets::new(Some(backend))
            .with_timing(std::time::Duration::from_millis(500), std::time::Duration::from_millis(20)),
    )
}

fn engine_with(secrets: Secrets) -> TestEngine {
    let dir = tempfile::tempdir().unwrap();
    let (events_tx, events) = unbounded_channel();
    let (internal_tx, internal) = unbounded_channel();
    let paths = Paths::in_dir(dir.path());
    let history = crate::history::History::load(&paths).0;
    let parts = EngineParts {
        paths,
        env: Environment::default(),
        protocols: Vec::new(),
        local_fs: Arc::new(porthmos_lfs::LocalFs::new(dir.path().to_path_buf())),
        transfers: TransferSettings::default(),
        bookmarks: Bookmarks::default(),
        secrets,
        history,
        edit: crate::config::settings::EditSettings::default(),
        publish_interval: std::time::Duration::ZERO,
        persist_interval: std::time::Duration::ZERO,
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
        group: None,
        tags: Vec::new(),
        saved_password: false,
        in_keyring: Vec::new(),
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
                protocol: Arc::new(porthmos_vfs::testing::FakeProtocol::new(FakeFs::new())),
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

    pub(crate) async fn settle(&mut self) {
        while let Ok(Some(done)) =
            tokio::time::timeout(std::time::Duration::from_millis(300), self.internal.recv()).await
        {
            self.engine.handle_internal(done);
        }
    }

    pub(crate) fn assert_secret_nowhere(&self, secret: &str, events: &[Event]) {
        let mut folders = vec![self.dir.path().to_path_buf()];
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = item.path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    let bytes = std::fs::read(&path).unwrap();
                    let found = bytes.windows(secret.len()).any(|window| window == secret.as_bytes());
                    assert!(!found, "{} holds the secret", path.display());
                }
            }
        }
        let printed = format!("{events:?}");
        assert!(!printed.contains(secret), "an event shows the secret: {printed}");
    }

    pub(crate) async fn run_internal(&mut self) {
        let done = tokio::time::timeout(std::time::Duration::from_secs(2), self.internal.recv())
            .await
            .expect("an internal event within two seconds")
            .expect("the internal channel is open");
        self.engine.handle_internal(done);
    }
}

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;

    fn make_writer(&'a self) -> LogBuffer {
        self.clone()
    }
}

pub(crate) fn capture_logs(run: impl FnOnce()) -> String {
    let buffer = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(buffer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let bytes = buffer.0.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}
