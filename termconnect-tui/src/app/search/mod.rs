use super::*;

impl App {
    pub(super) fn open_search_screen(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let Some(location) = self.active_location() else {
            self.notifications.push(Severity::Warning, "Connect to a remote server first");
            return;
        };

        self.search = Some(SearchSession { view: SearchView::new(), location });
        self.screen = Screen::Search;
    }

    pub(super) fn apply_search_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.search.is_some() {
                self.core.send(Command::CancelSearch);
            }
            return;
        }

        let Some(session) = self.search.as_mut() else {
            return;
        };

        match session.view.handle_key(key) {
            SearchOutcome::Cancel => {
                self.search = None;
                self.core.send(Command::CancelSearch);
                self.screen = Screen::Files;
                self.open_next_conflict_prompt();
            }
            SearchOutcome::PatternChanged => self.restart_search(),
            SearchOutcome::Open => self.open_selected_search_result(),
            SearchOutcome::Pending => {}
        }
    }

    fn restart_search(&mut self) {
        let Some(session) = self.search.as_mut() else {
            return;
        };
        session.view.start();
        let pattern = session.view.pattern.clone();
        let location = session.location;
        if pattern.is_empty() {
            self.core.send(Command::CancelSearch);
            return;
        }

        let root = self.panel(location).map(|panel| panel.path().to_path_buf()).unwrap_or_default();
        self.core.send(Command::Search { location, root, pattern: format!("*{pattern}*") });
    }

    fn open_selected_search_result(&mut self) {
        let Some(session) = self.search.as_ref() else {
            return;
        };
        let Some(entry) = session.view.selected_entry().cloned() else {
            return;
        };
        let location = session.location;
        let parent = entry.path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| entry.path.clone());

        self.search = None;
        self.core.send(Command::CancelSearch);
        self.screen = Screen::Files;
        self.open_next_conflict_prompt();
        self.active_panel = match location {
            Location::Local => ActivePanel::Local,
            Location::Session(id) => {
                self.sessions.activate(id);
                ActivePanel::Remote
            }
        };
        self.core.send(Command::List { location, path: Some(parent) });
    }
}

#[cfg(test)]
mod tests;
