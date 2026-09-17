use super::*;

use crate::connection::profile::ConnectionProfile;
use crate::tui::dialog::{FormDialog, FormField};

impl App {
    /// Hands the terminal over to the system `ssh` client, per the
    /// roadmap's Terminal Lifecycle: leave the alternate screen, let `ssh`
    /// take stdin/stdout/stderr, wait for it to exit, then reinitialize the
    /// TUI. Nothing else in the event loop runs while `ssh` has the
    /// terminal, by design — that's the whole point of the handover.
    pub(super) async fn launch_ssh_terminal(
        &mut self,
        terminal: &mut ratatui::Terminal<Backend>,
    ) -> Result<()> {
        let Some(entry) = self.sessions.active().map(|session| session.entry.clone()) else {
            self.notifications
                .push(Severity::Warning, "Connect to a server first");
            return Ok(());
        };

        tui::restore()?;

        let ssh_result = tokio::task::spawn_blocking(move || terminal::run(&entry)).await;

        // `tui::init` builds a brand-new `Terminal` with an empty internal
        // buffer, so the next `draw` repaints everything on its own —
        // deliberately not calling `.clear()` here, since it queries the
        // cursor position via a DSR escape sequence that some terminals
        // (or terminals mid-handover right after `ssh` exits) may not
        // answer in time, which would turn a cosmetic no-op into a crash.
        *terminal = tui::init()?;

        match ssh_result {
            Ok(Ok(status)) if !status.success() => {
                self.notifications
                    .push(Severity::Error, format!("ssh exited with status {status}"));
            }
            Ok(Ok(_)) => {}
            Ok(Err(io_err)) => {
                self.notifications
                    .push(Severity::Error, format!("Failed to launch ssh: {io_err}"));
            }
            Err(join_err) => {
                self.notifications
                    .push(Severity::Error, format!("ssh task failed: {join_err}"));
            }
        }

        Ok(())
    }

    pub(super) fn open_connections_screen(&mut self) {
        self.screen = Screen::Connections;

        match connection::list_all() {
            Ok(entries) => {
                self.connections = entries;
                self.connections_cursor = self
                    .connections_cursor
                    .min(self.connections.len().saturating_sub(1));
            }
            Err(err) => self.notifications.push(Severity::Error, err.to_string()),
        }
    }

    pub(super) fn open_add_connection_dialog(&mut self) {
        if self.screen != Screen::Connections {
            return;
        }

        self.dialog = Some(build_connection_form("Add connection", None));
        self.pending_action = Some(PendingAction::AddConnection);
    }

    /// Only closes the dialog once `store::save` actually succeeds — a
    /// disk-level failure (e.g. an unwritable config directory) must leave
    /// the form open with the user's input intact, exactly like a
    /// validation failure does, rather than discarding it.
    pub(super) fn submit_add_connection(&mut self, values: Vec<String>) {
        let profile = match build_connection_profile(&values, None) {
            Ok(profile) => profile,
            Err(message) => {
                self.set_form_error(message);
                return;
            }
        };

        match connection::store::save(&profile) {
            Ok(()) => {
                self.dialog = None;
                self.pending_action = None;
                self.open_connections_screen();
            }
            Err(err) => self.notifications.push(Severity::Error, err.to_string()),
        }
    }

    pub(super) fn open_edit_connection_dialog(&mut self) {
        if self.screen != Screen::Connections {
            return;
        }
        let Some(entry) = self.connections.get(self.connections_cursor).cloned() else {
            return;
        };
        if entry.source == ConnectionSource::SshConfig {
            self.notifications.push(
                Severity::Info,
                "This connection is defined in ~/.ssh/config and can't be edited here",
            );
            return;
        }

        self.dialog = Some(build_connection_form("Edit connection", Some(&entry)));
        self.pending_action = Some(PendingAction::EditConnection { original: entry });
    }

    /// Same close-only-on-success rule as `submit_add_connection`. A rename
    /// (name changed) deletes the old profile first — if that delete fails,
    /// the form stays open rather than going on to save under the new name,
    /// so the user doesn't end up with two saved copies from one confirm.
    pub(super) fn submit_edit_connection(&mut self, values: Vec<String>) {
        let Some(PendingAction::EditConnection { original }) = &self.pending_action else {
            return;
        };
        let original = original.clone();

        let profile = match build_connection_profile(&values, Some(&original)) {
            Ok(profile) => profile,
            Err(message) => {
                self.set_form_error(message);
                return;
            }
        };

        if profile.name != original.name
            && let Err(err) = connection::store::delete(&original.name)
        {
            self.notifications.push(Severity::Error, err.to_string());
            return;
        }

        match connection::store::save(&profile) {
            Ok(()) => {
                self.dialog = None;
                self.pending_action = None;
                self.open_connections_screen();
            }
            Err(err) => self.notifications.push(Severity::Error, err.to_string()),
        }
    }

    pub(super) fn set_form_error(&mut self, message: String) {
        if let Some(Dialog::Form(form)) = self.dialog.as_mut() {
            form.error = Some(message);
        }
    }

    pub(super) fn connect_to_selected(&mut self) {
        let Some(entry) = self.connections.get(self.connections_cursor).cloned() else {
            return;
        };

        if let Some(session) = self.sessions.by_host(&entry.name) {
            self.sessions.activate(session.id);
            return;
        }

        self.connection_status = ConnectionStatus::Connecting(entry.name.clone());
        let tx = self.connect_tx.clone();
        tokio::spawn(run_connect(entry, tx));
    }

    /// Disconnects the connection under the connections-screen cursor, if
    /// it's connected — `F8`'s counterpart to `Enter`'s connect/switch.
    pub(super) fn disconnect_selected(&mut self) {
        let Some(entry) = self.connections.get(self.connections_cursor) else {
            return;
        };
        let name = entry.name.clone();
        let Some(id) = self.sessions.by_host(&name).map(|session| session.id) else {
            return;
        };

        self.sessions.remove(id);
        self.session_resources.remove(&id);

        // Clean up anything the disconnected session left behind in the
        // transfer queue: an in-flight transfer would otherwise fail with a
        // raw I/O error once its `Arc<SftpSession>` handle is dropped (and
        // get retried up to 3 times, each retry its own error
        // notification), and each still-queued job would otherwise trickle
        // through `maybe_start_next_transfer`'s per-job
        // "session disconnected" failure path one at a time, each pushing
        // its own notification. Handle both here in one pass and report a
        // single aggregated notification instead.
        let mut affected = self
            .transfers
            .fail_queued_for_session(id, "session disconnected");
        if self
            .transfers
            .active()
            .is_some_and(|job| job.session_id == id)
        {
            self.cancel_active_transfer();
            affected += 1;
        }
        if affected > 0 {
            let plural = if affected == 1 { "" } else { "s" };
            self.notifications.push(
                Severity::Info,
                format!("{affected} transfer{plural} cancelled \u{2014} session disconnected"),
            );
        }

        self.notifications
            .push(Severity::Info, format!("Disconnected from {name}"));
    }

    pub(super) fn apply_connect_event(&mut self, event: ConnectEvent) {
        match event {
            ConnectEvent::Connected {
                entry,
                handle,
                sftp,
            } => {
                self.connection_status = ConnectionStatus::Disconnected;
                let placeholder_panel = PanelState::from_listing(PathBuf::from("/"), Vec::new());
                let id = self.sessions.insert(entry, placeholder_panel);
                self.session_resources.insert(
                    id,
                    SessionResources {
                        handle: Arc::new(handle),
                        sftp: Arc::new(sftp),
                    },
                );
                self.spawn_initial_remote_listing(id);
            }
            ConnectEvent::NeedsPassword {
                name,
                username,
                respond_to,
            } => {
                self.pending_password = Some(respond_to);
                self.dialog = Some(Dialog::TextInput(TextInputDialog::new_masked(format!(
                    "Password for {username}@{name}"
                ))));
                self.pending_action = Some(PendingAction::SubmitPassword);
            }
            ConnectEvent::Failed { message } => {
                self.connection_status = ConnectionStatus::Failed(message.clone());
                self.notifications.push(Severity::Error, message);
            }
        }
    }

    pub(super) fn apply_panel_event(&mut self, event: PanelEvent) {
        match event {
            PanelEvent::Listed {
                session_id,
                path,
                entries,
            } => {
                if let Some(session) = self.sessions.by_id_mut(session_id) {
                    session.panel.replace_listing(path, entries);
                }
            }
            PanelEvent::Failed {
                session_id,
                message,
            } => {
                if self.sessions.by_id_mut(session_id).is_some() {
                    self.notifications.push(Severity::Error, message);
                } else {
                    tracing::debug!(
                        "dropping stale panel error for session {session_id}: {message}"
                    );
                }
            }
        }
    }

    fn spawn_initial_remote_listing(&mut self, session_id: u64) {
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            let home = match sftp.canonicalize(".").await {
                Ok(home) => home,
                Err(err) => {
                    tracing::debug!("{err:?}");
                    let err = anyhow::Error::from(err);
                    let _ = tx.send(PanelEvent::Failed {
                        session_id,
                        message: errors::user_message("Unable to list home directory", &err),
                    });
                    return;
                }
            };
            relist(&sftp, session_id, PathBuf::from(home), &tx).await;
        });
    }

    /// Refreshes the *active* session's panel — used for user-driven
    /// navigation/refresh, where "the remote panel" unambiguously means
    /// whichever session is focused.
    pub(super) fn spawn_remote_list(&mut self, path: PathBuf) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        self.spawn_remote_list_for(session_id, path);
    }

    /// Refreshes a specific session's panel by id — used when the session
    /// that needs refreshing isn't necessarily the active one (a finished
    /// transfer targets whichever session it was queued against).
    pub(super) fn spawn_remote_list_for(&mut self, session_id: u64, path: PathBuf) {
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();
        tokio::spawn(async move { relist(&sftp, session_id, path, &tx).await });
    }

    pub(super) fn spawn_remote_mkdir(&mut self, name: String) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        let Some(session) = self.sessions.active() else {
            return;
        };
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let dir_path = session.panel.path().to_path_buf();
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            let target = path_to_remote_string(&dir_path.join(&name));
            if let Err(err) = filesystem::remote::create_directory(&sftp, &target).await {
                tracing::debug!("{err:?}");
                let _ = tx.send(PanelEvent::Failed {
                    session_id,
                    message: errors::user_message("Unable to create directory", &err),
                });
                return;
            }
            relist(&sftp, session_id, dir_path, &tx).await;
        });
    }

    pub(super) fn spawn_remote_rename(&mut self, new_name: String) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        let Some(session) = self.sessions.active() else {
            return;
        };
        let Some(current_name) = session.panel.current_entry_name() else {
            return;
        };
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let dir_path = session.panel.path().to_path_buf();
        let from = path_to_remote_string(&dir_path.join(current_name));
        let to = path_to_remote_string(&dir_path.join(&new_name));
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            if let Err(err) = filesystem::remote::rename(&sftp, &from, &to).await {
                tracing::debug!("{err:?}");
                let _ = tx.send(PanelEvent::Failed {
                    session_id,
                    message: errors::user_message("Unable to rename", &err),
                });
                return;
            }
            relist(&sftp, session_id, dir_path, &tx).await;
        });
    }

    pub(super) fn spawn_remote_delete(&mut self) {
        let Some(session_id) = self.sessions.active_id() else {
            return;
        };
        let Some(session) = self.sessions.active() else {
            return;
        };
        let Some(resources) = self.session_resources.get(&session_id) else {
            return;
        };
        let targets = session.panel.targets();
        let dir_path = session.panel.path().to_path_buf();
        let sftp = resources.sftp.clone();
        let tx = self.panel_tx.clone();

        tokio::spawn(async move {
            for target in targets {
                let target_str = path_to_remote_string(&target);
                if let Err(err) = filesystem::remote::delete(&sftp, &target_str).await {
                    tracing::debug!("{err:?}");
                    let _ = tx.send(PanelEvent::Failed {
                        session_id,
                        message: errors::user_message("Unable to delete", &err),
                    });
                    return;
                }
            }
            relist(&sftp, session_id, dir_path, &tx).await;
        });
    }
}

/// Lists `path` over SFTP and reports the outcome — the tail end of every
/// remote panel operation (navigate, mkdir, rename, delete all finish by
/// refreshing the listing, just like their local counterparts do).
async fn relist(
    sftp: &SftpSession,
    session_id: u64,
    path: PathBuf,
    tx: &mpsc::UnboundedSender<PanelEvent>,
) {
    let path_str = path_to_remote_string(&path);
    match filesystem::remote::list(sftp, &path_str).await {
        Ok(entries) => {
            let _ = tx.send(PanelEvent::Listed {
                session_id,
                path,
                entries,
            });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(PanelEvent::Failed {
                session_id,
                message: errors::user_message(format!("Unable to list {}", path.display()), &err),
            });
        }
    }
}

/// Runs a full connection attempt in the background: dial, verify the host
/// key, authenticate in the roadmap's priority order (agent, key file,
/// password), then open the SFTP subsystem — reporting progress back over
/// `tx` so the UI never blocks on network I/O. A password prompt is
/// requested via a one-shot round-trip embedded in
/// [`ConnectEvent::NeedsPassword`].
async fn run_connect(entry: ConnectionEntry, tx: mpsc::UnboundedSender<ConnectEvent>) {
    let mut handle = match connection::client::connect(&entry.host, entry.port).await {
        Ok(handle) => handle,
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(format!("Unable to connect to {}", entry.name), &err),
            });
            return;
        }
    };

    match connection::client::authenticate_non_interactive(&mut handle, &entry).await {
        Ok(true) => {
            finish_connect(entry, handle, &tx).await;
            return;
        }
        Ok(false) => {}
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(
                    format!("Authentication error for {}", entry.name),
                    &err,
                ),
            });
            return;
        }
    }

    let (respond_to, password_rx) = oneshot::channel();
    let request = ConnectEvent::NeedsPassword {
        name: entry.name.clone(),
        username: entry.username.clone(),
        respond_to,
    };
    if tx.send(request).is_err() {
        return;
    }

    let Ok(password) = password_rx.await else {
        let _ = tx.send(ConnectEvent::Failed {
            message: "Connection cancelled".to_string(),
        });
        return;
    };

    match connection::client::authenticate_password(&mut handle, &entry.username, &password).await {
        Ok(true) => finish_connect(entry, handle, &tx).await,
        Ok(false) => {
            let _ = tx.send(ConnectEvent::Failed {
                message: format!("Authentication failed for {}", entry.name),
            });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(
                    format!("Authentication error for {}", entry.name),
                    &err,
                ),
            });
        }
    }
}

/// Opens the SFTP subsystem on a freshly-authenticated session and reports
/// the finished connection, or a failure if SFTP itself couldn't start.
async fn finish_connect(
    entry: ConnectionEntry,
    handle: russh::client::Handle<TermConnectHandler>,
    tx: &mpsc::UnboundedSender<ConnectEvent>,
) {
    match connection::client::open_sftp(&handle).await {
        Ok(sftp) => {
            let _ = tx.send(ConnectEvent::Connected {
                entry,
                handle,
                sftp,
            });
        }
        Err(err) => {
            tracing::debug!("{err:?}");
            let _ = tx.send(ConnectEvent::Failed {
                message: errors::user_message(
                    format!("Connected to {} but failed to start SFTP", entry.name),
                    &err,
                ),
            });
        }
    }
}

fn build_connection_form(title: &str, existing: Option<&ConnectionEntry>) -> Dialog {
    let (name, host, port, username, password) = match existing {
        Some(entry) => (
            entry.name.clone(),
            entry.host.clone(),
            entry.port.to_string(),
            entry.username.clone(),
            entry.password.clone().unwrap_or_default(),
        ),
        None => (
            String::new(),
            String::new(),
            "22".to_string(),
            String::new(),
            String::new(),
        ),
    };

    Dialog::Form(FormDialog::new(
        title,
        vec![
            FormField::new("Name", name),
            FormField::new("Host", host),
            FormField::new("Port", port),
            FormField::new("Username", username),
            FormField::new_masked("Password", password),
        ],
    ))
}

fn build_connection_profile(
    values: &[String],
    preserve_from: Option<&ConnectionEntry>,
) -> Result<ConnectionProfile, String> {
    let [name, host, port, username, password] = values else {
        return Err("Unexpected number of fields".to_string());
    };
    let name = name.trim();
    let host = host.trim();
    let username = username.trim();

    if name.is_empty() {
        return Err("Name can't be empty".to_string());
    }
    if host.is_empty() {
        return Err("Host can't be empty".to_string());
    }
    if username.is_empty() {
        return Err("Username can't be empty".to_string());
    }
    let port: u16 = port
        .trim()
        .parse()
        .map_err(|_| "Port must be a number from 1-65535".to_string())?;
    let password = if password.is_empty() {
        None
    } else {
        Some(password.clone())
    };

    Ok(ConnectionProfile {
        name: name.to_string(),
        host: host.to_string(),
        port,
        username: username.to_string(),
        identity_file: preserve_from.and_then(|entry| entry.identity_file.clone()),
        remote_path: preserve_from.and_then(|entry| entry.remote_path.clone()),
        password,
    })
}

#[cfg(test)]
#[path = "../../tests/app/connections_test.rs"]
mod tests;
