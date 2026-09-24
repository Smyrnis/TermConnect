use std::{path::PathBuf, sync::Arc};

use porthmos_vfs::FileSystem;
use tokio::sync::mpsc::UnboundedSender;

use super::{
    super::{Engine, Event, Location},
    failure_message,
};
use crate::Severity;

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

impl Engine {
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
