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
                _ => self.open_delete_dialog(),
            },
            Action::Copy => self.start_copy(),
            Action::CancelTransfer => self.cancel_all_copies(),
            Action::OpenConnections => self.open_connections_screen(),
            Action::AddConnection => self.open_add_connection_dialog(),
            Action::DeleteConnection => self.open_delete_connection_dialog(),
            Action::BookmarkHere => self.open_bookmark_add_dialog(),
            Action::OpenBookmarks => self.open_bookmarks_dialog(),
            Action::OpenSearch => self.open_search_screen(),
            Action::CycleSession => {
                if self.screen == Screen::Files {
                    self.sessions.cycle();
                }
            }
            Action::Help => self.help_visible = true,
            Action::Back => self.handle_back(),
            Action::Up
            | Action::Down
            | Action::ToggleSelect
            | Action::Open
            | Action::Refresh
            | Action::ToggleHidden
            | Action::CycleSort => {
                self.apply_screen_action(action);
            }
            // Handled specially in `run`, which has the `&mut Terminal`
            // this needs to suspend/resume the TUI around `ssh`.
            Action::OpenTerminal => {}
            Action::Noop => {}
        }
    }

    /// `Esc`: dismisses a persistent error notification first, if one is
    /// showing; otherwise falls back to its usual meaning of closing the
    /// dialog/returning to the Files screen.
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
        }
    }

    /// Actions that operate on whichever panel is focused.
    fn apply_panel_action(&mut self, action: Action) {
        match self.active_panel {
            ActivePanel::Local => self.apply_local_panel_action(action),
            ActivePanel::Remote => self.apply_remote_panel_action(action),
        }
    }

    fn apply_local_panel_action(&mut self, action: Action) {
        let result = match action {
            Action::Up => {
                self.local.move_cursor(-1);
                Ok(())
            }
            Action::Down => {
                self.local.move_cursor(1);
                Ok(())
            }
            Action::ToggleSelect => {
                self.local.toggle_selection();
                Ok(())
            }
            Action::Open => self.local.open_selected(),
            Action::Refresh => self.local.refresh(),
            Action::ToggleHidden => {
                self.local.toggle_hidden();
                Ok(())
            }
            Action::CycleSort => {
                self.local.cycle_sort();
                Ok(())
            }
            _ => Ok(()),
        };

        self.set_status(result);
    }

    /// Remote navigation/selection is instant (pure state), but anything
    /// that needs a fresh listing (`Open`, `Refresh`) has to go over the
    /// network, so it's dispatched to a background task instead of run
    /// inline — see `spawn_remote_list`.
    fn apply_remote_panel_action(&mut self, action: Action) {
        let target_path = {
            let Some(session) = self.sessions.active_mut() else {
                return;
            };
            match action {
                Action::Up => {
                    session.panel.move_cursor(-1);
                    return;
                }
                Action::Down => {
                    session.panel.move_cursor(1);
                    return;
                }
                Action::ToggleSelect => {
                    session.panel.toggle_selection();
                    return;
                }
                Action::ToggleHidden => {
                    session.panel.toggle_hidden();
                    return;
                }
                Action::CycleSort => {
                    session.panel.cycle_sort();
                    return;
                }
                Action::Open => session.panel.target_path_for_open(),
                Action::Refresh => Some(session.panel.path().to_path_buf()),
                _ => return,
            }
        };

        if let Some(path) = target_path {
            self.spawn_remote_list(path);
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
#[path = "../../tests/app/actions_test.rs"]
mod tests;
