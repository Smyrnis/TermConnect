use super::*;

impl App {
    pub(super) fn start_copy(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        match self.active_panel {
            ActivePanel::Local => self.copy_to_remote(),
            ActivePanel::Remote => self.copy_to_local(),
        }
    }

    fn copy_to_remote(&mut self) {
        let Some(session) = self.sessions.active() else {
            self.notifications.push(Severity::Warning, "Connect to a remote server first");
            return;
        };
        let entries = self.local.target_entries();
        if entries.is_empty() {
            return;
        }
        self.core.send(Command::Copy {
            from: Location::Local,
            entries,
            to: Location::Session(session.id),
            dest_dir: session.panel.path().to_path_buf(),
        });
    }

    fn copy_to_local(&mut self) {
        let Some(session) = self.sessions.active() else {
            return;
        };
        let entries = session.panel.target_entries();
        if entries.is_empty() {
            return;
        }
        self.core.send(Command::Copy {
            from: Location::Session(session.id),
            entries,
            to: Location::Local,
            dest_dir: self.local.path().to_path_buf(),
        });
    }
}

#[cfg(test)]
mod tests;
