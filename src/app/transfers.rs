use super::*;

impl App {
    pub(super) fn start_copy(&mut self) {
        if self.screen != Screen::Files {
            return;
        }

        match self.active_panel {
            ActivePanel::Local => self.enqueue_uploads(),
            ActivePanel::Remote => self.enqueue_downloads(),
        }

        self.fill_transfer_slots();
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
        let cancel = Arc::new(AtomicBool::new(false));
        self.planning.push(PlanningScan { batch_id, session_id, display_name, cancel: cancel.clone() });

        let tx = self.transfer_tx.clone();
        tokio::spawn(async move {
            let result = transfer::plan::plan_directory_copy(direction, entries, &dest_dir, &sftp, &cancel).await;
            let event = match result {
                Ok(transfer::plan::PlanOutcome::Ready(plan)) => TransferEvent::PlanReady { batch_id, session_id, direction, plan },
                Ok(transfer::plan::PlanOutcome::Cancelled) => TransferEvent::PlanCancelled { batch_id, session_id, direction },
                Err(err) => {
                    tracing::debug!("{err:?}");
                    TransferEvent::PlanFailed { batch_id, message: errors::user_message("Copy failed", &err) }
                }
            };
            let _ = tx.send(event);
        });
    }

    pub(super) fn fill_transfer_slots(&mut self) {
        loop {
            let startable = self.transfers.startable(self.max_parallel);
            if startable.is_empty() {
                return;
            }
            for id in startable {
                self.start_transfer(id);
            }
        }
    }

    fn start_transfer(&mut self, id: u64) {
        let Some(job) = self.transfers.get(id) else {
            return;
        };
        let session_id = job.session_id;
        let display_name = job.display_name.clone();

        let Some(resources) = self.session_resources.get(&session_id) else {
            if let Some(job) = self.transfers.get_mut(id) {
                job.status = JobStatus::Failed("session disconnected".to_string());
            }
            self.notifications.push(Severity::Error, format!("Transfer failed: {display_name} \u{2014} session disconnected"));
            self.refresh_transfer_destination(id);
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

        let cancel = Arc::new(AtomicBool::new(false));
        self.transfer_cancels.insert(id, cancel.clone());

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
                    TransferEvent::Failed { id, message: errors::user_message(format!("Transfer failed: {display_name}"), &err) }
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
                self.transfer_cancels.remove(&id);
                if let Some(job) = self.transfers.get_mut(id) {
                    job.status = match outcome {
                        TransferOutcome::Completed => JobStatus::Completed,
                        TransferOutcome::Cancelled => JobStatus::Cancelled,
                    };
                }
                self.refresh_transfer_destination(id);
                self.fill_transfer_slots();
            }
            TransferEvent::Failed { id, message } => {
                let cancelled = self.transfer_cancels.remove(&id).is_some_and(|cancel| cancel.load(Ordering::Relaxed));
                if let Some(job) = self.transfers.get_mut(id) {
                    job.status = if cancelled { JobStatus::Cancelled } else { JobStatus::Failed(message.clone()) };
                }
                let retried = !cancelled && self.transfers.retry_or_give_up(id);
                if !retried {
                    if !cancelled {
                        self.notifications.push(Severity::Error, message);
                    }
                    self.refresh_transfer_destination(id);
                }
                self.fill_transfer_slots();
            }
            TransferEvent::PlanReady { batch_id, session_id, direction, plan } => {
                self.clear_planning(batch_id);

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
            TransferEvent::PlanCancelled { batch_id, session_id, direction } => {
                self.clear_planning(batch_id);
                let session_still_connected = self.sessions.iter().any(|session| session.id == session_id);
                if session_still_connected {
                    self.notifications.push(Severity::Info, "Copy cancelled");
                }
                self.refresh_destination_panel(session_id, direction);
            }
        }
    }

    fn apply_plan_ready(&mut self, batch_id: u64, session_id: u64, direction: Direction, plan: transfer::plan::DirectoryPlan) {
        if plan.files.is_empty() {
            self.refresh_destination_panel(session_id, direction);
        }

        for file in plan.files {
            self.transfers.enqueue(session_id, direction, file.local_path, file.remote_path, file.display_name, file.size, Some(batch_id));
        }
        if plan.skipped_symlinks > 0 {
            let plural = if plan.skipped_symlinks == 1 { "" } else { "s" };
            self.notifications.push(Severity::Warning, format!("Skipped {} symlink{plural}", plan.skipped_symlinks));
        }
        self.fill_transfer_slots();
    }

    fn clear_planning(&mut self, batch_id: u64) {
        self.planning.retain(|scan| scan.batch_id != batch_id);
    }

    fn refresh_transfer_destination(&mut self, id: u64) {
        let Some(job) = self.transfers.get(id) else {
            return;
        };
        let session_id = job.session_id;
        let direction = job.direction;

        if self.transfers.has_pending(session_id, direction) {
            return;
        }

        self.refresh_destination_panel(session_id, direction);
    }

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

    pub(super) fn cancel_all_copies(&mut self) {
        for scan in &self.planning {
            scan.cancel.store(true, Ordering::Relaxed);
        }
        for cancel in self.transfer_cancels.values() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.transfers.cancel_all_queued();
    }

    pub(super) fn cancel_session_transfers(&mut self, session_id: u64) -> usize {
        let session_scans: Vec<&PlanningScan> = self.planning.iter().filter(|scan| scan.session_id == session_id).collect();
        for scan in &session_scans {
            scan.cancel.store(true, Ordering::Relaxed);
        }
        let active_ids = self.transfers.active_ids_for_session(session_id);
        for id in &active_ids {
            if let Some(cancel) = self.transfer_cancels.get(id) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        session_scans.len() + active_ids.len()
    }
}

#[cfg(test)]
#[path = "../../tests/app/transfers_test.rs"]
mod tests;
