use super::*;

impl App {
    /// Copies the focused panel's selection (or the entry under the
    /// cursor) to the other panel's current directory — upload if LOCAL is
    /// focused, download if REMOTE is focused. Direction and source/target
    /// panel follow the roadmap's rule: "determined by the active/source
    /// panel."
    pub(super) fn start_copy(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        match self.active_panel {
            ActivePanel::Local => self.enqueue_uploads(),
            ActivePanel::Remote => self.enqueue_downloads(),
        }

        self.maybe_start_next_transfer();
    }

    fn enqueue_uploads(&mut self) {
        let Some(session) = self.sessions.active() else {
            self.notifications.push(Severity::Warning, "Connect to a remote server first");
            return;
        };
        let session_id = session.id;
        let remote_dir = session.panel.path().to_path_buf();
        let entries = self.local.target_entries();
        self.enqueue_transfers(session_id, Direction::Upload, entries, remote_dir);
    }

    fn enqueue_downloads(&mut self) {
        let Some(session) = self.sessions.active() else {
            return;
        };
        let session_id = session.id;
        let local_dir = self.local.path().to_path_buf();
        let entries = session.panel.target_entries();
        self.enqueue_transfers(session_id, Direction::Download, entries, local_dir);
    }

    fn enqueue_transfers(&mut self, session_id: u64, direction: Direction, entries: Vec<Entry>, dest_dir: PathBuf) {
        if entries.is_empty() {
            return;
        }

        if entries.iter().any(|entry| entry.is_dir) {
            self.start_directory_copy(session_id, direction, entries, dest_dir);
            return;
        }

        for entry in entries {
            let (local_path, remote_path) = match direction {
                Direction::Upload => (entry.path.clone(), path_to_remote_string(&dest_dir.join(&entry.name))),
                Direction::Download => (dest_dir.join(&entry.name), path_to_remote_string(&entry.path)),
            };

            self.transfers.enqueue(session_id, direction, local_path, remote_path, entry.name, entry.size, None);
        }
    }

    /// Kicks off a directory copy's planning phase: looks up the session's
    /// SFTP handle synchronously (so a disconnected session fails fast
    /// with a notification, exactly like `maybe_start_next_transfer`
    /// already does for a job whose session vanished — no `tokio::spawn`
    /// happens on that path), then spawns `plan_directory_copy` and sends
    /// its outcome back as `PlanReady`/`PlanFailed`.
    fn start_directory_copy(&mut self, session_id: u64, direction: Direction, entries: Vec<Entry>, dest_dir: PathBuf) {
        let Some(resources) = self.session_resources.get(&session_id) else {
            self.notifications.push(Severity::Error, "Copy failed: session disconnected");
            return;
        };
        let sftp = resources.sftp.clone();

        let batch_id = self.transfers.start_batch();
        let display_name = match entries.as_slice() {
            [entry] => entry.name.clone(),
            _ => format!("{} items", entries.len()),
        };
        self.planning = Some((batch_id, display_name));

        let tx = self.transfer_tx.clone();
        tokio::spawn(async move {
            let event = match transfer::plan::plan_directory_copy(direction, entries, &dest_dir, &sftp).await {
                Ok(plan) => TransferEvent::PlanReady { batch_id, session_id, direction, plan },
                Err(err) => {
                    tracing::debug!("{err:?}");
                    TransferEvent::PlanFailed { batch_id, message: errors::user_message("Copy failed", &err) }
                }
            };
            let _ = tx.send(event);
        });
    }

    pub(super) fn maybe_start_next_transfer(&mut self) {
        let Some(id) = self.transfers.next_to_run() else {
            return;
        };
        let Some(job) = self.transfers.get(id) else {
            return;
        };
        let session_id = job.session_id;
        let display_name = job.display_name.clone();

        let Some(resources) = self.session_resources.get(&session_id) else {
            if let Some(job) = self.transfers.get_mut(id) {
                job.status = JobStatus::Failed("session disconnected".to_string());
            }
            self.notifications
                .push(Severity::Error, format!("Transfer failed: {display_name} \u{2014} session disconnected"));
            self.maybe_start_next_transfer();
            return;
        };
        let sftp = resources.sftp.clone();

        let Some(job) = self.transfers.get_mut(id) else {
            return;
        };
        job.status = JobStatus::InProgress;
        job.attempts += 1;
        let direction = job.direction;
        let local_path = job.local_path.clone();
        let remote_path = job.remote_path.clone();
        let display_name = job.display_name.clone();

        let cancel = Arc::new(AtomicBool::new(false));
        self.active_transfer_cancel = Some(cancel.clone());

        let tx = self.transfer_tx.clone();
        tokio::spawn(async move {
            let progress_tx = tx.clone();
            let result = transfer::run(direction, &local_path, &remote_path, &sftp, &cancel, move |transferred| {
                let _ = progress_tx.send(TransferEvent::Progress { id, transferred });
            })
            .await;

            let event = match result {
                Ok(outcome) => TransferEvent::Finished { id, outcome },
                Err(err) => {
                    tracing::debug!("{err:?}");
                    TransferEvent::Failed {
                        id,
                        message: errors::user_message(format!("Transfer failed: {display_name}"), &err),
                    }
                }
            };
            let _ = tx.send(event);
        });
    }

    pub(super) fn apply_transfer_event(&mut self, event: TransferEvent) {
        match event {
            TransferEvent::Progress { id, transferred } => {
                if let Some(job) = self.transfers.get_mut(id) {
                    job.transferred_bytes = transferred;
                }
            }
            TransferEvent::Finished { id, outcome } => {
                self.active_transfer_cancel = None;
                if let Some(job) = self.transfers.get_mut(id) {
                    job.status = match outcome {
                        TransferOutcome::Completed => JobStatus::Completed,
                        TransferOutcome::Cancelled => JobStatus::Cancelled,
                    };
                }
                self.refresh_transfer_destination(id);
                self.maybe_start_next_transfer();
            }
            TransferEvent::Failed { id, message } => {
                self.active_transfer_cancel = None;
                if let Some(job) = self.transfers.get_mut(id) {
                    job.status = JobStatus::Failed(message.clone());
                }
                if !self.transfers.retry_or_give_up(id) {
                    self.notifications.push(Severity::Error, message);
                }
                self.maybe_start_next_transfer();
            }
            TransferEvent::PlanReady { batch_id, session_id, direction, plan } => {
                self.clear_planning(batch_id);

                // Planning is async, so the session can disconnect while it
                // was running. Enqueueing anyway would let every file fail
                // one at a time through maybe_start_next_transfer's own
                // "session disconnected" path, each pushing its own
                // non-expiring Error notification. Fail fast with one
                // instead, and leave the queue untouched.
                if !self.session_resources.contains_key(&session_id) {
                    self.notifications.push(Severity::Error, "Copy failed: session disconnected");
                    return;
                }

                self.apply_plan_ready(batch_id, session_id, direction, plan);
            }
            TransferEvent::PlanFailed { batch_id, message } => {
                self.clear_planning(batch_id);
                self.notifications.push(Severity::Error, message);
            }
        }
    }

    /// The part of `PlanReady` handling that runs once the session's still
    /// known to be present: enqueues every planned file under `batch_id`
    /// and warns once about any skipped symlinks. Split out from the event
    /// arm above so it stays unit-testable without a live `SftpSession` —
    /// the session-presence guard's own "session present" branch needs one
    /// to exercise, which no unit test in this codebase can construct, but
    /// this half has no such dependency.
    fn apply_plan_ready(
        &mut self, batch_id: u64, session_id: u64, direction: Direction, plan: transfer::plan::DirectoryPlan,
    ) {
        if plan.files.is_empty() {
            // Planning may still have created the destination directory
            // (e.g. an empty source directory) even though nothing got
            // enqueued — without this, no job ever runs to trigger the
            // usual per-job refresh, so the new directory stays invisible
            // until a manual Ctrl+R.
            self.refresh_destination_panel(session_id, direction);
        }

        for file in plan.files {
            self.transfers.enqueue(
                session_id,
                direction,
                file.local_path,
                file.remote_path,
                file.display_name,
                file.size,
                Some(batch_id),
            );
        }
        if plan.skipped_symlinks > 0 {
            let plural = if plan.skipped_symlinks == 1 { "" } else { "s" };
            self.notifications.push(Severity::Warning, format!("Skipped {} symlink{plural}", plan.skipped_symlinks));
        }
        self.maybe_start_next_transfer();
    }

    /// Clears `self.planning` only if it's still tracking `batch_id`. Plain
    /// `self.planning = None` would let one directory copy's completion
    /// wipe out a second, still-scanning copy's "Scanning..." indicator if
    /// the user pressed `F5` again on another directory while the first
    /// one's planning was still running.
    fn clear_planning(&mut self, batch_id: u64) {
        if self.planning.as_ref().is_some_and(|(id, _)| *id == batch_id) {
            self.planning = None;
        }
    }

    /// Refreshes whichever panel just received a file, so the new listing
    /// is visible without a manual `Ctrl+R`. For a job that's part of a
    /// batch, this only actually refreshes once nothing else from the same
    /// batch is still queued to run next — refreshing after every file in
    /// a large directory copy would otherwise issue a full remote listing
    /// (and churn the panel's cursor/selection) after every single file.
    fn refresh_transfer_destination(&mut self, id: u64) {
        let Some(job) = self.transfers.get(id) else {
            return;
        };
        let session_id = job.session_id;
        let direction = job.direction;

        if let Some(batch_id) = job.batch_id {
            let batch_still_running = self
                .transfers
                .next_to_run()
                .and_then(|next_id| self.transfers.get(next_id))
                .is_some_and(|next_job| next_job.batch_id == Some(batch_id));
            if batch_still_running {
                return;
            }
        }

        self.refresh_destination_panel(session_id, direction);
    }

    /// Does the actual panel refresh for `session_id`/`direction` — shared
    /// by `refresh_transfer_destination` (per-job, batch-aware) and
    /// `apply_transfer_event`'s `PlanReady` zero-file case (nothing ever
    /// gets enqueued for an empty directory, so the per-job path above
    /// never runs).
    fn refresh_destination_panel(&mut self, session_id: u64, direction: Direction) {
        match direction {
            Direction::Upload => {
                if let Some(session) = self.sessions.by_id_mut(session_id) {
                    let path = session.panel.path().to_path_buf();
                    self.spawn_remote_list_for(session_id, path);
                }
            }
            Direction::Download => {
                let _ = self.local.refresh();
            }
        }
    }

    pub(super) fn cancel_active_transfer(&mut self) {
        if let Some(cancel) = &self.active_transfer_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(job) = self.transfers.active()
            && let Some(batch_id) = job.batch_id
        {
            self.transfers.cancel_batch(batch_id);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/app/transfers_test.rs"]
mod tests;
