#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use porthmos_core::{
    Answer, Command, Core, CoreHandle, Entry, Event, Location, Paths, Question,
    config::{Settings, settings::TransferSettings},
    transfer::{
        conflicts::{ConflictInfo, ConflictPolicy, Resolution},
        rows::RowState,
    },
};
use porthmos_scp::Scp;
use porthmos_ssh::testing::{Options, PASSWORD, SshServer, USER};
use tokio::sync::mpsc::UnboundedReceiver;

pub struct Rig {
    pub core: CoreHandle,
    events: UnboundedReceiver<Event>,
    pub server: SshServer,
    local: tempfile::TempDir,
    pub session: u64,
    pub max_active: usize,
    pub notices: Vec<String>,
    _config: tempfile::TempDir,
}

pub fn data(size: usize, seed: u8) -> Vec<u8> {
    (0..size).map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

pub async fn rig(max_parallel: usize, policy: ConflictPolicy) -> Rig {
    let server = SshServer::start(Options::default()).await;
    let config = tempfile::tempdir().unwrap();
    let local = tempfile::tempdir().unwrap();
    let paths = Paths::in_dir(config.path());
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    let profile = format!(
        "[connections.box]\nprotocol = \"scp\"\nhost = \"127.0.0.1\"\nport = {}\nusername = \"{USER}\"\npassword = \"{PASSWORD}\"\n",
        server.port
    );
    std::fs::write(paths.connections_file(), profile).unwrap();
    let settings =
        Settings { transfers: TransferSettings { max_parallel, on_conflict: policy }, ..Settings::default() };
    let scp = Scp::default().with_connect_options(server.connect_options());
    let (core, events) = Core::builder()
        .paths(paths)
        .settings(settings)
        .local_home(local.path().to_path_buf())
        .protocol(Arc::new(scp))
        .start()
        .unwrap();
    let mut rig = Rig { core, events, server, local, session: 0, max_active: 0, notices: Vec::new(), _config: config };
    rig.core.send(Command::Connect { profile: "box".into() });
    loop {
        match rig.event().await {
            Event::Question { request_id, question: Question::TrustHostKey { .. } } => {
                rig.core.send(Command::Answer { request_id, answer: Some(Answer::Confirmed) })
            }
            Event::Connected { session, .. } => {
                rig.session = session;
                return rig;
            }
            Event::ConnectFailed { message, .. } => panic!("connect failed: {message}"),
            _ => {}
        }
    }
}

impl Rig {
    pub async fn event(&mut self) -> Event {
        let event = tokio::time::timeout(Duration::from_secs(30), self.events.recv())
            .await
            .expect("an event within 30 seconds")
            .expect("the core is running");
        match &event {
            Event::TransfersChanged(snapshot) => self.max_active = self.max_active.max(snapshot.active.len()),
            Event::Notice { message, .. } => self.notices.push(message.clone()),
            _ => {}
        }
        event
    }

    pub fn remote(&self, name: &str) -> PathBuf {
        self.server.root.path().join(name)
    }

    pub fn local(&self, name: &str) -> PathBuf {
        self.local.path().join(name)
    }

    fn entry(path: &Path, is_dir: bool) -> Entry {
        let size = if is_dir { 0 } else { std::fs::metadata(path).unwrap().len() };
        Entry {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path: path.to_path_buf(),
            is_dir,
            size,
            permissions: None,
        }
    }

    pub fn upload(&self, names: &[(&str, bool)]) {
        let entries = names.iter().map(|(name, is_dir)| Self::entry(&self.local(name), *is_dir)).collect();
        self.core.send(Command::Copy {
            from: Location::Local,
            entries,
            to: Location::Session(self.session),
            dest_dir: self.server.root.path().to_path_buf(),
        });
    }

    pub fn download(&self, names: &[(&str, bool)]) {
        let entries = names.iter().map(|(name, is_dir)| Self::entry(&self.remote(name), *is_dir)).collect();
        self.core.send(Command::Copy {
            from: Location::Session(self.session),
            entries,
            to: Location::Local,
            dest_dir: self.local.path().to_path_buf(),
        });
    }

    pub async fn settle(&mut self, answers: impl Fn(&[ConflictInfo]) -> Option<Vec<Resolution>>) -> Vec<RowState> {
        let mut started = false;
        loop {
            match self.event().await {
                Event::ConflictsFound { batch_id, files } => {
                    self.core.send(Command::ResolveConflicts { batch_id, answers: answers(&files) });
                }
                Event::TransfersChanged(snapshot) => {
                    let busy = !snapshot.active.is_empty()
                        || snapshot.queued > 0
                        || !snapshot.scanning.is_empty()
                        || snapshot.awaiting_answers > 0;
                    started |= !snapshot.rows.is_empty();
                    if !busy && started {
                        return snapshot.rows.iter().map(|row| row.state).collect();
                    }
                }
                _ => {}
            }
        }
    }

    pub async fn drain(&mut self) {
        while tokio::time::timeout(Duration::from_millis(300), self.events.recv()).await.is_ok_and(|event| {
            if let Some(Event::Notice { message, .. }) = &event {
                self.notices.push(message.clone());
            }
            event.is_some()
        }) {}
    }
}
