use super::*;
use crate::filesystem::search;

impl App {
    pub(super) fn open_search_screen(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        let target = match self.active_panel {
            ActivePanel::Local => SearchTarget::Local,
            ActivePanel::Remote => {
                if self.sessions.active().is_none() {
                    self.notifications
                        .push(Severity::Warning, "Connect to a remote server first");
                    return;
                }
                SearchTarget::Remote
            }
        };

        self.search = Some(SearchSession {
            view: SearchView::new(),
            target,
            cancel: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
        });
        self.screen = Screen::Search;
    }

    pub(super) fn apply_search_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(session) = &self.search {
                session.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }

        let Some(session) = self.search.as_mut() else {
            return;
        };

        match session.view.handle_key(key) {
            SearchOutcome::Cancel => {
                if let Some(session) = self.search.take() {
                    session.cancel.store(true, Ordering::Relaxed);
                }
                self.screen = Screen::Files;
            }
            SearchOutcome::PatternChanged => self.restart_search(),
            SearchOutcome::Open => self.open_selected_search_result(),
            SearchOutcome::Pending => {}
        }
    }

    /// Cancels any in-flight (or still-debouncing) search and schedules a
    /// new one for the current pattern — called on every keystroke that
    /// changes the pattern. The actual search doesn't dispatch immediately:
    /// it waits out `SEARCH_DEBOUNCE` first (see `wait_out_search_debounce`)
    /// so a burst of keystrokes coalesces into one search per pause instead
    /// of one remote `find`/local walk per keystroke.
    fn restart_search(&mut self) {
        let Some(session) = self.search.as_mut() else {
            return;
        };
        session.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        session.cancel = cancel.clone();
        session.view.start();

        let generation = session.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation_state = session.generation.clone();

        let pattern = session.view.pattern.clone();
        if pattern.is_empty() {
            return;
        }
        // The search box is a plain substring filter, not a glob editor —
        // wrap the typed text so `glob_match`'s anchored matching (see
        // `filesystem::search`) behaves like "contains" instead of
        // requiring an exact filename match.
        let glob_pattern = format!("*{pattern}*");

        let tx = self.search_tx.clone();
        match session.target {
            SearchTarget::Local => {
                let root = self.local.path().to_path_buf();
                tokio::spawn(async move {
                    if !wait_out_search_debounce(&cancel, &generation_state, generation).await {
                        return;
                    }
                    search::search_local(root, glob_pattern, tx, cancel).await;
                });
            }
            SearchTarget::Remote => {
                let Some(session_id) = self.sessions.active_id() else {
                    return;
                };
                let Some(resources) = self.session_resources.get(&session_id) else {
                    return;
                };
                let sftp = resources.sftp.clone();
                let handle = resources.handle.clone();
                let root = self
                    .sessions
                    .active()
                    .map(|session| session.panel.path().to_path_buf())
                    .unwrap_or_default();
                let root_str = path_to_remote_string(&root);
                tokio::spawn(async move {
                    if !wait_out_search_debounce(&cancel, &generation_state, generation).await {
                        return;
                    }
                    search::search_remote(&handle, &sftp, root_str, glob_pattern, tx, cancel).await;
                });
            }
        }
    }

    pub(super) fn apply_search_event(&mut self, event: SearchEvent) {
        let Some(session) = self.search.as_mut() else {
            return;
        };
        match event {
            SearchEvent::Found(entry) => session.view.push_result(entry),
            SearchEvent::Done { truncated } => session.view.finish(truncated),
            SearchEvent::Failed(message) => {
                session.view.finish(false);
                self.notifications.push(Severity::Error, message);
            }
        }
    }

    /// Closes the search screen and navigates the target panel to the
    /// selected result's parent directory.
    fn open_selected_search_result(&mut self) {
        let Some(session) = self.search.as_ref() else {
            return;
        };
        let Some(entry) = session.view.selected_entry().cloned() else {
            return;
        };
        let target_panel = match session.target {
            SearchTarget::Local => ActivePanel::Local,
            SearchTarget::Remote => ActivePanel::Remote,
        };
        let parent = entry
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| entry.path.clone());

        if let Some(session) = self.search.take() {
            session.cancel.store(true, Ordering::Relaxed);
        }
        self.screen = Screen::Files;
        self.active_panel = target_panel;

        match target_panel {
            ActivePanel::Local => {
                let result = self.local.navigate_to(parent);
                self.set_status(result);
            }
            ActivePanel::Remote => self.spawn_remote_list(parent),
        }
    }
}

/// Waits out `SEARCH_DEBOUNCE`, then reports whether the search that
/// scheduled this wait is still the one to run: `false` means either it was
/// cancelled (e.g. the pattern changed again, or the search screen closed)
/// or a newer pattern change superseded it (`generation` no longer matches
/// `expected`), in which case the caller should skip dispatching the actual
/// search entirely.
async fn wait_out_search_debounce(
    cancel: &Arc<AtomicBool>,
    generation: &Arc<AtomicU64>,
    expected: u64,
) -> bool {
    tokio::time::sleep(SEARCH_DEBOUNCE).await;
    !cancel.load(Ordering::Relaxed) && generation.load(Ordering::Relaxed) == expected
}

#[cfg(test)]
#[path = "../../tests/app/search_test.rs"]
mod tests;
