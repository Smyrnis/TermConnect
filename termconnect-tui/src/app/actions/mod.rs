use super::*;

impl App {
    pub(super) fn apply_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::SwitchPanel => self.active_panel.toggle(),
            Action::Mkdir => self.open_mkdir_dialog(),
            Action::Rename => match self.screen {
                Screen::Connections => self.open_edit_connection_dialog(),
                _ => self.open_rename_dialog(),
            },
            Action::Delete => match self.screen {
                Screen::Connections => self.disconnect_selected(),
                Screen::Transfers => self.cancel_selected_row(),
                _ => self.open_delete_dialog(),
            },
            Action::Copy => self.start_copy(),
            Action::CancelTransfer => self.core.send(Command::CancelAllTransfers),
            Action::OpenConnections => self.open_connections_screen(),
            Action::AddConnection => self.open_add_connection_dialog(),
            Action::DeleteConnection => self.open_delete_connection_dialog(),
            Action::BookmarkHere => self.open_bookmark_add_dialog(),
            Action::OpenBookmarks => self.open_bookmarks_dialog(),
            Action::OpenSearch => self.open_search_screen(),
            Action::OpenTransfers => self.open_transfers_screen(),
            Action::CycleSession => {
                if self.screen == Screen::Files {
                    self.sessions.cycle();
                }
            }
            Action::Help => self.help_visible = true,
            Action::Back => self.handle_back(),
            Action::OpenTerminal => self.request_shell(),
            Action::Up
            | Action::Down
            | Action::ToggleSelect
            | Action::Open
            | Action::Refresh
            | Action::ToggleHidden
            | Action::CycleSort => {
                self.apply_screen_action(action);
            }
            Action::Noop => {}
        }
    }

    fn handle_back(&mut self) {
        let showing_error = matches!(self.notifications.current().map(|n| n.severity), Some(Severity::Error));
        if showing_error {
            self.notifications.dismiss_current();
        } else {
            self.screen = Screen::Files;
        }
    }

    fn apply_screen_action(&mut self, action: Action) {
        match self.screen {
            Screen::Files => self.apply_panel_action(action),
            Screen::Connections => self.apply_connections_action(action),
            Screen::Search => {}
            Screen::Transfers => self.apply_transfers_action(action),
        }
    }

    fn apply_panel_action(&mut self, action: Action) {
        let Some(location) = self.active_location() else {
            return;
        };
        let panel = match location {
            Location::Local => &mut self.local,
            Location::Session(_) => match self.sessions.active_mut() {
                Some(session) => &mut session.panel,
                None => return,
            },
        };
        let target_path = match action {
            Action::Up => {
                panel.move_cursor(-1);
                return;
            }
            Action::Down => {
                panel.move_cursor(1);
                return;
            }
            Action::ToggleSelect => {
                panel.toggle_selection();
                return;
            }
            Action::ToggleHidden => {
                panel.toggle_hidden();
                return;
            }
            Action::CycleSort => {
                panel.cycle_sort();
                return;
            }
            Action::Open => panel.target_path_for_open(),
            Action::Refresh => Some(panel.path().to_path_buf()),
            _ => return,
        };

        if let Some(path) = target_path {
            self.core.send(Command::List { location, path: Some(path) });
        }
    }

    fn apply_connections_action(&mut self, action: Action) {
        match action {
            Action::Up => {
                self.connections_cursor = self.connections_cursor.saturating_sub(1);
            }
            Action::Down if self.connections_cursor + 1 < self.connections.len() => {
                self.connections_cursor += 1;
            }
            Action::Down => {}
            Action::Open => self.connect_to_selected(),
            Action::Refresh => self.open_connections_screen(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
