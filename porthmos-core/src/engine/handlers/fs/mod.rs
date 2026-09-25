use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use porthmos_vfs::{FileSystem, ProtocolError};
use tokio::sync::mpsc::UnboundedSender;

use super::{
    super::{Engine, Event, Location, SessionId},
    failure_message,
};
use crate::{Severity, user_message};

fn send_failure(events: &UnboundedSender<Event>, message: String) {
    let _ = events.send(Event::Notice { severity: Severity::Error, message });
}

async fn list_into(fs: Arc<dyn FileSystem>, location: Location, path: Option<PathBuf>, events: UnboundedSender<Event>) {
    let path = match path {
        Some(path) => path,
        None => match fs.home().await {
            Ok(home) => home,
            Err(err) => {
                tracing::debug!("{err:?}");
                send_failure(&events, failure_message(location, "Unable to list home directory", &err));
                return;
            }
        },
    };
    match fs.list(&path).await {
        Ok(entries) => {
            let _ = events.send(Event::Listed { location, path, entries });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            send_failure(&events, failure_message(location, format!("Unable to list {}", path.display()), &err));
        }
    }
}

fn start_directory(home: &Path, requested: &str) -> PathBuf {
    match requested.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(under_home) if under_home.starts_with('/') => home.join(under_home.trim_start_matches('/')),
        _ => home.join(requested),
    }
}

async fn resolve_start_directory(fs: &dyn FileSystem, requested: &str) -> Result<PathBuf, ProtocolError> {
    if Path::new(requested).is_absolute() {
        return Ok(PathBuf::from(requested));
    }
    Ok(start_directory(&fs.home().await?, requested))
}

async fn list_start_into(
    fs: Arc<dyn FileSystem>, session: SessionId, name: String, requested: String, events: UnboundedSender<Event>,
) {
    let location = Location::Session(session);
    let path = match resolve_start_directory(fs.as_ref(), &requested).await {
        Ok(path) => path,
        Err(err) => {
            tracing::debug!("{err:?}");
            send_failure(&events, failure_message(location, "Unable to list home directory", &err));
            return;
        }
    };
    match fs.list(&path).await {
        Ok(entries) => {
            let _ = events.send(Event::Listed { location, path, entries });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            let context = format!("Unable to open {} on {name}, showing the home directory instead", path.display());
            let _ = events.send(Event::Notice { severity: Severity::Warning, message: user_message(context, &err) });
            list_into(fs, location, None, events).await;
        }
    }
}

impl Engine {
    pub(crate) fn open_start_directory(&mut self, session: SessionId) {
        let Some(live) = self.sessions.get(&session) else {
            return;
        };
        match live.entry.start_path() {
            Some(requested) => {
                tokio::spawn(list_start_into(
                    live.fs.clone(),
                    session,
                    live.name.clone(),
                    requested.to_string(),
                    self.events.clone(),
                ));
            }
            None => self.list(Location::Session(session), None),
        }
    }

    fn filesystem_or_drop(&self, location: Location) -> Option<Arc<dyn FileSystem>> {
        let fs = self.fs_for(location);
        if fs.is_none() {
            tracing::debug!("dropping a file operation for a session that is gone: {location:?}");
        }
        fs
    }

    pub(crate) fn list(&mut self, location: Location, path: Option<PathBuf>) {
        let Some(fs) = self.filesystem_or_drop(location) else {
            return;
        };
        tokio::spawn(list_into(fs, location, path, self.events.clone()));
    }

    pub(crate) fn create_dir(&mut self, location: Location, path: PathBuf) {
        let Some(fs) = self.filesystem_or_drop(location) else {
            return;
        };
        let events = self.events.clone();
        tokio::spawn(async move {
            match fs.create_dir(&path).await {
                Ok(()) => {
                    let _ = events.send(Event::LocationChanged { location });
                }
                Err(err) => {
                    tracing::debug!("{err:?}");
                    send_failure(&events, failure_message(location, "Unable to create directory", &err));
                }
            }
        });
    }

    pub(crate) fn rename(&mut self, location: Location, from: PathBuf, to: PathBuf) {
        let Some(fs) = self.filesystem_or_drop(location) else {
            return;
        };
        let events = self.events.clone();
        tokio::spawn(async move {
            match fs.rename(&from, &to).await {
                Ok(()) => {
                    let _ = events.send(Event::LocationChanged { location });
                }
                Err(err) => {
                    tracing::debug!("{err:?}");
                    send_failure(&events, failure_message(location, "Unable to rename", &err));
                }
            }
        });
    }

    pub(crate) fn delete(&mut self, location: Location, paths: Vec<PathBuf>) {
        let Some(fs) = self.filesystem_or_drop(location) else {
            return;
        };
        let events = self.events.clone();
        tokio::spawn(async move {
            for path in paths {
                if let Err(err) = fs.delete(&path).await {
                    tracing::debug!("{err:?}");
                    send_failure(&events, failure_message(location, "Unable to delete", &err));
                    return;
                }
            }
            let _ = events.send(Event::LocationChanged { location });
        });
    }
}

#[cfg(test)]
mod tests;
