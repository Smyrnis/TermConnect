use std::path::PathBuf;

use super::super::{Engine, Event, Location};
use crate::{
    Severity,
    config::bookmarks::{self, Bookmark},
};

impl Engine {
    fn save_bookmarks(&self) {
        if let Err(err) = bookmarks::save_to(&self.paths.bookmarks_file(), &self.bookmarks) {
            self.notice(Severity::Error, err.to_string());
        }
    }

    fn refuse_protected_bookmarks(&self) -> bool {
        let protected = self.bookmarks.is_protected();
        if protected {
            self.notice(
                Severity::Error,
                "Bookmarks can't be changed: bookmarks.toml could not be read or set aside, so it was left alone",
            );
        }
        protected
    }

    pub(crate) fn publish_bookmarks(&self) {
        self.emit(Event::Bookmarks(self.bookmarks.iter().cloned().collect()));
    }

    pub(crate) fn add_bookmark(&mut self, label: String, location: Location, path: PathBuf) {
        let host = match location {
            Location::Local => None,
            Location::Session(id) => match self.sessions.get(&id) {
                Some(session) => Some(session.name.clone()),
                None => {
                    self.notice(Severity::Warning, "Connect to a remote server first");
                    return;
                }
            },
        };

        if self.refuse_protected_bookmarks() {
            return;
        }
        self.bookmarks.add(Bookmark { label, path, host });
        self.save_bookmarks();
        self.publish_bookmarks();
    }

    pub(crate) fn remove_bookmark(&mut self, index: usize) {
        if self.refuse_protected_bookmarks() {
            return;
        }
        if let Some(removed) = self.bookmarks.remove(index) {
            self.save_bookmarks();
            self.notice(Severity::Info, format!("Removed bookmark \"{}\"", removed.label));
            self.publish_bookmarks();
        }
    }
}

#[cfg(test)]
mod tests;
