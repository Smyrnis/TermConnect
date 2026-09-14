use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{List, ListItem, ListState};

use crate::tui::panels::{PanelState, Row};

pub fn render_file_list(frame: &mut Frame, area: Rect, panel: &PanelState, is_active: bool) {
    let items: Vec<ListItem> = panel
        .rows()
        .iter()
        .map(|row| ListItem::new(row_label(row, is_selected(panel, row))))
        .collect();

    let mut highlight_style = Style::default().add_modifier(Modifier::REVERSED);
    if !is_active {
        highlight_style = highlight_style.add_modifier(Modifier::DIM);
    }

    let list = List::new(items).highlight_style(highlight_style);

    let mut state = ListState::default();
    if !panel.rows().is_empty() {
        state.select(Some(panel.cursor));
    }

    frame.render_stateful_widget(list, area, &mut state);
}

fn is_selected(panel: &PanelState, row: &Row) -> bool {
    match row {
        Row::Entry(entry) => panel.selected.contains(&entry.path),
        Row::Parent => false,
    }
}

fn row_label(row: &Row, selected: bool) -> String {
    match row {
        Row::Parent => "  ..".to_string(),
        Row::Entry(entry) => {
            let marker = if selected { '*' } else { ' ' };
            let suffix = if entry.is_dir { "/" } else { "" };
            format!("{marker} {}{suffix}", entry.name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::fs;

    #[test]
    fn renders_entry_names() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("readme.txt"), b"content").unwrap();
        let panel = PanelState::new(dir.path().to_path_buf()).unwrap();

        let backend = TestBackend::new(30, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_file_list(frame, frame.area(), &panel, true))
            .unwrap();

        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(content.contains("readme.txt"));
    }
}
