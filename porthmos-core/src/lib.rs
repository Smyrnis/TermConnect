#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

pub mod config;
mod engine;
pub mod error;
pub mod listing;
pub mod paths;
pub mod profiles;
mod protocol_info;
pub mod transfer;

pub use engine::{Command, Event, Location, RequestId, SessionId};
pub use error::{Severity, connect_failure_message, user_message};
pub use paths::{ConfigMigration, Paths};
pub use porthmos_vfs::{
    Answer, Choice, CommonField, ConnectionForm, DirItem, Entry, Environment, ErrorKind, FileKind, FileSystem,
    Metadata, OptionField, OptionKind, PortField, Protocol, ProtocolError, Question, SearchEvent, SearchQuery,
    ShellInvocation, Target, path_to_remote_string,
};
pub use protocol_info::ProtocolInfo;

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{
    config::Settings,
    engine::{Engine, EngineParts},
};

#[cfg(feature = "sftp")]
fn sftp_protocol() -> Option<Arc<dyn Protocol>> {
    Some(Arc::new(porthmos_sftp::Sftp))
}

#[cfg(not(feature = "sftp"))]
fn sftp_protocol() -> Option<Arc<dyn Protocol>> {
    None
}

#[cfg(feature = "ftp")]
fn ftp_protocol(paths: &Paths) -> Option<Arc<dyn Protocol>> {
    Some(Arc::new(porthmos_ftp::Ftp::new(paths.known_certificates_file())))
}

#[cfg(not(feature = "ftp"))]
fn ftp_protocol(_paths: &Paths) -> Option<Arc<dyn Protocol>> {
    None
}

#[cfg(feature = "webdav")]
fn webdav_protocol(paths: &Paths) -> Option<Arc<dyn Protocol>> {
    Some(Arc::new(porthmos_webdav::WebDav::new(paths.known_certificates_file())))
}

#[cfg(not(feature = "webdav"))]
fn webdav_protocol(_paths: &Paths) -> Option<Arc<dyn Protocol>> {
    None
}

#[cfg(feature = "s3")]
fn s3_protocol(paths: &Paths) -> Option<Arc<dyn Protocol>> {
    Some(Arc::new(porthmos_s3::S3::new(paths.known_certificates_file())))
}

#[cfg(not(feature = "s3"))]
fn s3_protocol(_paths: &Paths) -> Option<Arc<dyn Protocol>> {
    None
}

pub fn builtin_protocols(paths: &Paths) -> Vec<Arc<dyn Protocol>> {
    [sftp_protocol(), ftp_protocol(paths), webdav_protocol(paths), s3_protocol(paths)].into_iter().flatten().collect()
}

#[derive(Clone)]
pub struct CoreHandle {
    commands: UnboundedSender<Command>,
    protocols: Arc<[ProtocolInfo]>,
}

impl CoreHandle {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn protocols(&self) -> &[ProtocolInfo] {
        &self.protocols
    }

    pub fn detached() -> (CoreHandle, UnboundedReceiver<Command>) {
        Self::detached_with(Vec::new())
    }

    pub fn detached_with(protocols: Vec<ProtocolInfo>) -> (CoreHandle, UnboundedReceiver<Command>) {
        let (commands, receiver) = unbounded_channel();
        (CoreHandle { commands, protocols: protocols.into() }, receiver)
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
        let protocols = self.protocols.unwrap_or_else(|| builtin_protocols(&paths));
        let infos: Arc<[ProtocolInfo]> =
            protocols.iter().map(|protocol| ProtocolInfo::from_protocol(protocol.as_ref())).collect();
        let parts = EngineParts {
            paths,
            protocols,
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
        Ok((CoreHandle { commands, protocols: infos }, event_receiver))
    }
}

#[cfg(test)]
mod tests;
