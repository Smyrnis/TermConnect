use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    Local,
    Remote,
}

impl ActivePanel {
    pub fn toggle(&mut self) {
        *self = match self {
            ActivePanel::Local => ActivePanel::Remote,
            ActivePanel::Remote => ActivePanel::Local,
        };
    }
}

pub fn render_panel(frame: &mut Frame, area: Rect, title: &str, is_active: bool) {
    let border_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    frame.render_widget(block, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn toggle_switches_between_local_and_remote() {
        let mut panel = ActivePanel::Local;
        panel.toggle();
        assert_eq!(panel, ActivePanel::Remote);
        panel.toggle();
        assert_eq!(panel, ActivePanel::Local);
    }

    #[test]
    fn render_panel_draws_the_given_title() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                render_panel(frame, Rect::new(0, 0, 20, 5), "LOCAL", false);
            })
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("LOCAL"));
    }
}
