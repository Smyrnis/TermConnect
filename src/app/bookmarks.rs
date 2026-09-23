use super::*;

impl App {
    pub(super) fn open_bookmark_add_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let default_label =
            self.active_panel_path().file_name().and_then(|name| name.to_str()).unwrap_or("bookmark").to_string();

        self.dialog = Some(Dialog::TextInput(TextInputDialog::new("Bookmark name", default_label)));
        self.pending_action = Some(PendingAction::AddBookmark);
    }

    fn active_panel_path(&self) -> PathBuf {
        match self.active_panel {
            ActivePanel::Local => self.local.path().to_path_buf(),
            ActivePanel::Remote => {
                self.sessions.active().map(|session| session.panel.path().to_path_buf()).unwrap_or_default()
            }
        }
    }

    pub(super) fn add_bookmark(&mut self, label: String) {
        let host = match self.active_panel {
            ActivePanel::Local => None,
            ActivePanel::Remote => match self.sessions.active() {
                Some(session) => Some(session.entry.name.clone()),
                None => {
                    self.notifications.push(Severity::Warning, "Connect to a remote server first");
                    return;
                }
            },
        };

        let path = self.active_panel_path();
        self.bookmarks.add(config::bookmarks::Bookmark { label, path, host });
        self.save_bookmarks();
    }

    pub(super) fn open_bookmarks_dialog(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let items: Vec<String> = self
            .bookmarks
            .iter()
            .map(|bookmark| match &bookmark.host {
                Some(host) => format!("{} \u{2014} {} [{host}]", bookmark.label, bookmark.path.display()),
                None => format!("{} \u{2014} {}", bookmark.label, bookmark.path.display()),
            })
            .collect();

        self.dialog = Some(Dialog::List(ListDialog::new("Bookmarks", items).removable(true)));
    }

    pub(super) fn navigate_to_bookmark(&mut self, index: usize) {
        let Some(bookmark) = self.bookmarks.get(index).cloned() else {
            return;
        };

        match bookmark.host {
            None => {
                self.active_panel = ActivePanel::Local;
                let result = self.local.navigate_to(bookmark.path);
                self.set_status(result);
            }
            Some(host) => {
                let Some(session_id) = self.sessions.by_host(&host).map(|session| session.id) else {
                    self.notifications.push(Severity::Warning, format!("Connect to {host} first"));
                    return;
                };
                self.sessions.activate(session_id);
                self.active_panel = ActivePanel::Remote;
                self.spawn_remote_list(bookmark.path);
            }
        }
    }

    pub(super) fn save_bookmarks(&mut self) {
        if let Some(path) = self.bookmarks_path.clone() {
            let result = config::bookmarks::save_to(&path, &self.bookmarks);
            self.set_status(result);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/bookmarks_test.rs"]
mod tests;
