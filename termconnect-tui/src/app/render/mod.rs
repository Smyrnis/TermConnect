use termconnect_core::transfer::{ActiveJob, rows::percent_of};

use super::*;

impl App {
    pub(super) fn render(&self, frame: &mut Frame) {
        let (title_area, main_area, status_area) = layout::split_frame(frame.area());

        self.render_title(frame, title_area);

        match self.screen {
            Screen::Files => self.render_files(frame, main_area),
            Screen::Connections => self.render_connections(frame, main_area),
            Screen::Search => self.render_search(frame, main_area),
            Screen::Transfers => self.render_transfers(frame, main_area),
        }

        self.render_status(frame, status_area);

        if let Some(dialog) = &self.dialog {
            dialog.render(frame, frame.area());
        }

        if self.help_visible {
            help::render_help(frame, frame.area(), &self.key_bindings);
        }
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let status_text = match &self.connection_status {
            ConnectionStatus::Connecting(name) => format!("Connecting to {name}\u{2026}"),
            ConnectionStatus::Failed(message) if self.sessions.is_empty() => {
                format!("Connection failed: {message}")
            }
            _ => match self.sessions.active() {
                Some(session) => format!("{} \u{2014} SSH: Connected", session.name),
                None => "Not connected".to_string(),
            },
        };

        let text = format!("TermConnect{:>width$}", status_text, width = status_text.len() + 4);
        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_files(&self, frame: &mut Frame, area: Rect) {
        let (local_area, remote_area) = layout::split_panels(area);

        panel_view::render_panel(frame, local_area, "LOCAL", self.active_panel == ActivePanel::Local, &self.local);

        match self.sessions.active() {
            Some(session) => {
                if self.sessions.len() > 1 {
                    let (tabs_area, panel_area) = layout::split_remote_with_tabs(remote_area);
                    self.render_session_tabs(frame, tabs_area);
                    panel_view::render_panel(
                        frame,
                        panel_area,
                        "REMOTE",
                        self.active_panel == ActivePanel::Remote,
                        &session.panel,
                    );
                } else {
                    panel_view::render_panel(
                        frame,
                        remote_area,
                        "REMOTE",
                        self.active_panel == ActivePanel::Remote,
                        &session.panel,
                    );
                }
            }
            None => {
                let remote_title = match &self.connection_status {
                    ConnectionStatus::Connecting(name) => {
                        format!("REMOTE (connecting to {name}\u{2026})")
                    }
                    _ => "REMOTE".to_string(),
                };
                panel_view::render_placeholder(
                    frame,
                    remote_area,
                    &remote_title,
                    self.active_panel == ActivePanel::Remote,
                );
            }
        }
    }

    fn render_session_tabs(&self, frame: &mut Frame, area: Rect) {
        let active_id = self.sessions.active_id();
        let labels: Vec<String> = self
            .sessions
            .iter()
            .map(|session| {
                let marker = if Some(session.id) == active_id { '>' } else { ' ' };
                format!("{marker}{}", session.name)
            })
            .collect();
        frame.render_widget(Paragraph::new(labels.join("  ")), area);
    }

    fn render_connections(&self, frame: &mut Frame, area: Rect) {
        let connected_names: std::collections::HashSet<&str> =
            self.sessions.iter().map(|session| session.name.as_str()).collect();
        let active_name = self.sessions.active().map(|session| session.name.as_str());
        connections_list::render_connections_list(
            frame,
            area,
            &self.connections,
            self.connections_cursor,
            &connected_names,
            active_name,
        );
    }

    fn render_transfers(&self, frame: &mut Frame, area: Rect) {
        let copy_key = self.key_bindings.key_for(Action::Copy).map(input::format_key_spec);
        transfer_list::render_transfer_list(
            frame,
            area,
            self.transfer_rows(),
            self.transfers_cursor,
            copy_key.as_deref(),
        );
    }

    fn render_search(&self, frame: &mut Frame, area: Rect) {
        if let Some(session) = &self.search {
            search_view::render_search(frame, area, &session.view);
        }
    }

    pub(super) fn render_status(&self, frame: &mut Frame, area: Rect) {
        let (text, style) = match self.notifications.current() {
            Some(notification) => (notification.message.clone(), notification_style(notification.severity)),
            None => match self.waiting_for_answer_text().or_else(|| self.planning_status_text()) {
                Some(text) => (text, Style::default()),
                None => {
                    if self.transfers.active.is_empty() {
                        (build_hint_text(&self.key_bindings), Style::default())
                    } else {
                        (self.transfer_status_text(&self.transfers.active), Style::default())
                    }
                }
            },
        };

        frame.render_widget(Paragraph::new(text).style(style), area);
    }

    fn waiting_for_answer_text(&self) -> Option<String> {
        if self.dialog.is_some() {
            return None;
        }
        match self.conflict_prompts.len() {
            0 => None,
            1 => Some("A copy is waiting for your answer".to_string()),
            waiting => Some(format!("{waiting} copies are waiting for your answer")),
        }
    }

    fn planning_status_text(&self) -> Option<String> {
        match self.transfers.scanning.as_slice() {
            [] => None,
            [name] => Some(format!("Scanning {name}\u{2026}")),
            scans => Some(format!("Scanning {} copies\u{2026}", scans.len())),
        }
    }

    fn transfer_status_text(&self, active: &[ActiveJob]) -> String {
        let verb = transfer_verb(active);
        let queued = self.transfers.queued;
        let suffix = if queued > 0 { format!(" ({queued} queued)") } else { String::new() };

        if let [job] = active {
            return match job.batch_id.and_then(|batch_id| self.transfers.batches.get(&batch_id)) {
                Some(progress) => {
                    let percent = percent_of(progress.transferred_bytes, progress.total_bytes);
                    format!(
                        "{verb} {}: {}/{} files, {percent}%{suffix}",
                        job.display_name, progress.completed_files, progress.total_files
                    )
                }
                None => format!("{verb} {}: {}%{suffix}", job.display_name, job.percent),
            };
        }

        let count = active.len();
        let shared_batch = active
            .first()
            .and_then(|first| first.batch_id)
            .filter(|batch_id| active.iter().all(|job| job.batch_id == Some(*batch_id)));
        match shared_batch.and_then(|batch_id| self.transfers.batches.get(&batch_id)) {
            Some(progress) => {
                let percent = percent_of(progress.transferred_bytes, progress.total_bytes);
                format!(
                    "{verb} {count} files: {}/{} files, {percent}%{suffix}",
                    progress.completed_files, progress.total_files
                )
            }
            None => {
                let transferred: u64 = active.iter().map(|job| job.transferred_bytes).sum();
                let total: u64 = active.iter().map(|job| job.total_bytes).sum();
                format!("{verb} {count} files: {}%{suffix}", percent_of(transferred, total))
            }
        }
    }
}

fn transfer_verb(active: &[ActiveJob]) -> &'static str {
    if active.iter().all(|job| job.direction == Direction::Upload) {
        "Uploading"
    } else if active.iter().all(|job| job.direction == Direction::Download) {
        "Downloading"
    } else {
        "Transferring"
    }
}

fn notification_style(severity: Severity) -> Style {
    match severity {
        Severity::Info => Style::default(),
        Severity::Warning => Style::default().fg(Color::Yellow),
        Severity::Error => Style::default().fg(Color::Red),
    }
}

fn build_hint_text(bindings: &input::KeyBindings) -> String {
    let entries = [(Action::Help, "Help"), (Action::OpenConnections, "Connections"), (Action::Quit, "Quit")];

    entries
        .into_iter()
        .filter_map(|(action, label)| {
            bindings.key_for(action).map(|spec| format!("{} {label}", input::format_key_spec(spec)))
        })
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
mod tests;
