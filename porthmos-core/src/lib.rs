#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

pub mod config;
mod engine;
pub mod error;
pub mod listing;
pub mod paths;
pub mod profiles;
pub mod transfer;

pub use engine::{Command, Event, Location, RequestId, SessionId};
pub use error::{Severity, connect_failure_message, user_message};
pub use paths::{ConfigMigration, Paths};
pub use porthmos_vfs::{
    Answer, DirItem, Entry, Environment, ErrorKind, FileKind, FileSystem, Metadata, Protocol, ProtocolError, Question,
    SearchEvent, SearchQuery, ShellInvocation, Target, path_to_remote_string,
};

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    config::Settings,
    engine::{Engine, EngineParts},
};

pub fn builtin_protocols() -> Vec<Arc<dyn Protocol>> {
    #[cfg(feature = "sftp")]
    let protocols: Vec<Arc<dyn Protocol>> = vec![Arc::new(porthmos_sftp::Sftp)];
    #[cfg(not(feature = "sftp"))]
    let protocols: Vec<Arc<dyn Protocol>> = Vec::new();
    protocols
}

#[derive(Clone)]
pub struct CoreHandle {
    commands: UnboundedSender<Command>,
}

impl CoreHandle {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn detached() -> (CoreHandle, UnboundedReceiver<Command>) {
        let (commands, receiver) = unbounded_channel();
        (CoreHandle { commands }, receiver)
    }
}

pub struct Core;

impl Core {
    pub fn builder() -> CoreBuilder {
        CoreBuilder::default()
    }
}

#[derive(Default)]
pub struct CoreBuilder {
    paths: Option<Paths>,
    env: Environment,
    settings: Settings,
    protocols: Option<Vec<Arc<dyn Protocol>>>,
    local_home: Option<PathBuf>,
}

impl CoreBuilder {
    pub fn paths(mut self, paths: Paths) -> Self {
        self.paths = Some(paths);
        self
    }

    pub fn environment(mut self, env: Environment) -> Self {
        self.env = env;
        self
    }

    pub fn settings(mut self, settings: Settings) -> Self {
        self.settings = settings;
        self
    }

    pub fn protocol(mut self, protocol: Arc<dyn Protocol>) -> Self {
        self.protocols.get_or_insert_with(Vec::new).push(protocol);
        self
    }

    pub fn local_home(mut self, home: PathBuf) -> Self {
        self.local_home = Some(home);
        self
    }

    pub fn start(self) -> Result<(CoreHandle, UnboundedReceiver<Event>)> {
        let paths = self.paths.context("the core needs its configuration paths")?;
        let (bookmarks, bookmark_warnings) = config::bookmarks::load(&paths)?;
        let local_home = self.local_home.or_else(|| self.env.home.clone()).unwrap_or_else(|| PathBuf::from("/"));
        let parts = EngineParts {
            paths,
            protocols: self.protocols.unwrap_or_else(builtin_protocols),
            local_fs: Arc::new(porthmos_lfs::LocalFs::new(local_home)),
            env: self.env,
            transfers: self.settings.transfers,
            bookmarks,
        };

        let (events, event_receiver) = unbounded_channel();
        let (commands, command_receiver) = unbounded_channel();
        let (internal, internal_receiver) = unbounded_channel();
        let engine = Engine::new(parts, events.clone(), internal);
        for warning in bookmark_warnings {
            let _ = events.send(Event::Notice { severity: Severity::Warning, message: warning.0 });
        }
        engine.publish_bookmarks();
        tokio::spawn(engine.run(command_receiver, internal_receiver));
        Ok((CoreHandle { commands }, event_receiver))
    }
}
