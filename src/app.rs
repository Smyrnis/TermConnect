use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use ratatui::Frame;

use crate::tui::input::{self, Action};
use crate::tui::panels::{self, ActivePanel};
use crate::tui::{Backend, layout};

pub struct App {
    should_quit: bool,
    active_panel: ActivePanel,
}

impl App {
    pub fn new() -> Self {
        Self {
            should_quit: false,
            active_panel: ActivePanel::Local,
        }
    }

    pub async fn run(&mut self, terminal: &mut ratatui::Terminal<Backend>) -> Result<()> {
        let mut events = EventStream::new();

        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;

            if let Some(event) = events.next().await
                && let Event::Key(key) = event?
                && key.kind == KeyEventKind::Press
            {
                self.handle_action(input::map_key(key));
            }
        }

        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let (local_area, remote_area) = layout::split_panels(frame.area());
        panels::render_panel(
            frame,
            local_area,
            "LOCAL",
            self.active_panel == ActivePanel::Local,
        );
        panels::render_panel(
            frame,
            remote_area,
            "REMOTE",
            self.active_panel == ActivePanel::Remote,
        );
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::SwitchPanel => self.active_panel.toggle(),
            Action::Noop => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_action_sets_should_quit() {
        let mut app = App::new();
        app.handle_action(Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn switch_panel_action_toggles_active_panel() {
        let mut app = App::new();
        assert_eq!(app.active_panel, ActivePanel::Local);
        app.handle_action(Action::SwitchPanel);
        assert_eq!(app.active_panel, ActivePanel::Remote);
    }

    #[test]
    fn noop_action_does_not_change_state() {
        let mut app = App::new();
        app.handle_action(Action::Noop);
        assert!(!app.should_quit);
        assert_eq!(app.active_panel, ActivePanel::Local);
    }
}
