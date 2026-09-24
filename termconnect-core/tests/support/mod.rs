use std::{sync::Arc, time::Duration};

use termconnect_core::{Command, Core, CoreHandle, Event, Paths};
use termconnect_vfs::testing::{FakeFs, FakeProtocol};
use tokio::sync::mpsc::UnboundedReceiver;

pub struct Harness {
    pub core: CoreHandle,
    pub events: UnboundedReceiver<Event>,
    pub remote: FakeFs,
    pub local: tempfile::TempDir,
    _config: tempfile::TempDir,
}

pub fn start_with(protocol: impl FnOnce(FakeFs) -> FakeProtocol) -> Harness {
    let config = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(config.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        paths.connections_file(),
        "[connections.srv]\nprotocol = \"fake\"\nhost = \"h\"\nusername = \"u\"\n",
    )
    .unwrap();
    let remote = FakeFs::new();
    let (core, events) = Core::builder()
        .paths(paths)
        .local_home(local.path().to_path_buf())
        .protocol(Arc::new(protocol(remote.clone())))
        .start()
        .unwrap();
    Harness { core, events, remote, local, _config: config }
}

pub fn start() -> Harness {
    start_with(FakeProtocol::new)
}

impl Harness {
    pub async fn next<T>(&mut self, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            match tokio::time::timeout_at(deadline, self.events.recv()).await {
                Ok(Some(event)) => {
                    if let Some(value) = pick(&event) {
                        return value;
                    }
                    seen.push(format!("{event:?}"));
                }
                _ => panic!("expected event not received; saw: {seen:#?}"),
            }
        }
    }

    pub async fn connect(&mut self) -> u64 {
        self.core.send(Command::Connect { profile: "srv".into() });
        self.next(|event| match event {
            Event::Connected { session, .. } => Some(*session),
            _ => None,
        })
        .await
    }
}
