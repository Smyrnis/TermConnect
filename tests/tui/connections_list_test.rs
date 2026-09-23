use std::collections::HashSet;

use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::connection::{ConnectionEntry, ConnectionSource};

#[test]
fn renders_connection_names() {
    let entries = vec![ConnectionEntry {
        name: "production".to_string(),
        host: "server.example.com".to_string(),
        port: 22,
        username: "deploy".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }];

    let backend = TestBackend::new(60, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_connections_list(frame, frame.area(), &entries, 0, &HashSet::new(), None)).unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("production"));
    assert!(content.contains("deploy@server.example.com:22"));
}

fn entry(name: &str) -> ConnectionEntry {
    ConnectionEntry {
        name: name.to_string(),
        host: format!("{name}.example.com"),
        port: 22,
        username: "user".to_string(),
        identity_file: None,
        remote_path: None,
        password: None,
        source: ConnectionSource::Profile,
    }
}

fn render_rows(entries: &[ConnectionEntry], connected: &HashSet<&str>, active: Option<&str>) -> Vec<String> {
    let width = 60;
    let backend = TestBackend::new(width, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_connections_list(frame, frame.area(), entries, 0, connected, active)).unwrap();

    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

#[test]
fn multiple_connected_sessions_all_show_as_connected() {
    let entries = vec![entry("a"), entry("b"), entry("c")];
    let connected: HashSet<&str> = ["a", "b"].into_iter().collect();

    let rows = render_rows(&entries, &connected, Some("a"));
    let full_text = rows.join("\n");

    assert!(full_text.contains("* a ("));
    assert!(full_text.contains("+ b ("));
    assert!(full_text.contains("  c ("));
    assert!(!full_text.contains("* c ("));
    assert!(!full_text.contains("+ c ("));
}
