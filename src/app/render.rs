use super::*;

impl App {
    pub(super) fn render(&self, frame: &mut Frame) {
        let (title_area, main_area, status_area) = layout::split_frame(frame.area());

        self.render_title(frame, title_area);

        match self.screen {
            Screen::Files => self.render_files(frame, main_area),
            Screen::Connections => self.render_connections(frame, main_area),
            Screen::Search => self.render_search(frame, main_area),
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
                Some(session) => format!("{} \u{2014} SSH: Connected", session.entry.name),
                None => "Not connected".to_string(),
            },
        };

        let text = format!(
            "TermConnect{:>width$}",
            status_text,
            width = status_text.len() + 4
        );
        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_files(&self, frame: &mut Frame, area: Rect) {
        let (local_area, remote_area) = layout::split_panels(area);

        panels::render_panel(
            frame,
            local_area,
            "LOCAL",
            self.active_panel == ActivePanel::Local,
            &self.local,
        );

        match self.sessions.active() {
            Some(session) => {
                if self.sessions.len() > 1 {
                    let (tabs_area, panel_area) = layout::split_remote_with_tabs(remote_area);
                    self.render_session_tabs(frame, tabs_area);
                    panels::render_panel(
                        frame,
                        panel_area,
                        "REMOTE",
                        self.active_panel == ActivePanel::Remote,
                        &session.panel,
                    );
                } else {
                    panels::render_panel(
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
                panels::render_placeholder(
                    frame,
                    remote_area,
                    &remote_title,
                    self.active_panel == ActivePanel::Remote,
                );
            }
        }
    }

    /// A one-line strip of session host names, the active one marked with
    /// `>` — plain text rather than a styled tab widget, matching the rest
    /// of the app's low-frills rendering.
    fn render_session_tabs(&self, frame: &mut Frame, area: Rect) {
        let active_id = self.sessions.active_id();
        let labels: Vec<String> = self
            .sessions
            .iter()
            .map(|session| {
                let marker = if Some(session.id) == active_id {
                    '>'
                } else {
                    ' '
                };
                format!("{marker}{}", session.entry.name)
            })
            .collect();
        frame.render_widget(Paragraph::new(labels.join("  ")), area);
    }

    fn render_connections(&self, frame: &mut Frame, area: Rect) {
        let connected_names: std::collections::HashSet<&str> = self
            .sessions
            .iter()
            .map(|session| session.entry.name.as_str())
            .collect();
        let active_name = self
            .sessions
            .active()
            .map(|session| session.entry.name.as_str());
        connections_list::render_connections_list(
            frame,
            area,
            &self.connections,
            self.connections_cursor,
            &connected_names,
            active_name,
        );
    }

    fn render_search(&self, frame: &mut Frame, area: Rect) {
        if let Some(session) = &self.search {
            search_view::render_search(frame, area, &session.view);
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let (text, style) = match self.notifications.current() {
            Some(notification) => (
                notification.message.clone(),
                notification_style(notification.severity),
            ),
            None => match self.transfers.active() {
                Some(job) => (self.transfer_status_text(job), Style::default()),
                None => (build_hint_text(&self.key_bindings), Style::default()),
            },
        };

        frame.render_widget(Paragraph::new(text).style(style), area);
    }

    fn transfer_status_text(&self, job: &transfer::TransferJob) -> String {
        let verb = match job.direction {
            Direction::Upload => "Uploading",
            Direction::Download => "Downloading",
        };
        let queued = self.transfers.queued_count();
        let suffix = if queued > 0 {
            format!(" ({queued} queued)")
        } else {
            String::new()
        };
        format!(
            "{verb} {}: {}%{suffix}",
            job.display_name,
            job.progress_percent()
        )
    }
}

fn notification_style(severity: Severity) -> Style {
    match severity {
        Severity::Info => Style::default(),
        Severity::Warning => Style::default().fg(Color::Yellow),
        Severity::Error => Style::default().fg(Color::Red),
    }
}

/// Builds the key-hint line from the live bindings, so a remapped action
/// shows its new key instead of a hardcoded default.
fn build_hint_text(bindings: &input::KeyBindings) -> String {
    let entries = [
        (Action::Help, "Help"),
        (Action::OpenConnections, "Connections"),
        (Action::Quit, "Quit"),
    ];

    entries
        .into_iter()
        .filter_map(|(action, label)| {
            bindings
                .key_for(action)
                .map(|spec| format!("{} {label}", input::format_key_spec(spec)))
        })
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
#[path = "../../tests/app/render_test.rs"]
mod tests;
