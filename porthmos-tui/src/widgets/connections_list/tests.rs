use std::collections::HashSet;

use ratatui::{Terminal, backend::TestBackend};

use super::*;
use porthmos_core::profiles::{ConnectionEntry, ConnectionSource};

#[test]
fn renders_connection_names() {
    let entries = vec![ConnectionEntry {
        name: "production".to_string(),
        host: "server.example.com".to_string(),
        protocol: "sftp".to_string(),
        port: 22,
        username: "deploy".to_string(),
        options: std::collections::BTreeMap::new(),
        password: None,
        source: ConnectionSource::Profile,
    }];

    let backend = TestBackend::new(60, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_connections_list(frame, frame.area(), &entries, 0, &HashSet::new(), None, &[]))
        .unwrap();

    let content: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    assert!(content.contains("production"));
    assert!(content.contains("deploy@server.example.com:22"));
}

fn entry(name: &str) -> ConnectionEntry {
    ConnectionEntry {
        name: name.to_string(),
        host: format!("{name}.example.com"),
        protocol: "sftp".to_string(),
        port: 22,
        username: "user".to_string(),
        options: std::collections::BTreeMap::new(),
        password: None,
        source: ConnectionSource::Profile,
    }
}

fn render_rows(entries: &[ConnectionEntry], connected: &HashSet<&str>, active: Option<&str>) -> Vec<String> {
    let width = 60;
    let backend = TestBackend::new(width, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_connections_list(frame, frame.area(), entries, 0, connected, active, &[])).unwrap();

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

    assert!(full_text.contains("* sftp a ("));
    assert!(full_text.contains("+ sftp b ("));
    assert!(full_text.contains("  sftp c ("));
    assert!(!full_text.contains("* sftp c ("));
    assert!(!full_text.contains("+ sftp c ("));
}

#[test]
fn each_row_shows_its_protocol_name_or_raw_id_when_unavailable() {
    let mut sftp = entry("alpha");
    sftp.protocol = "sftp".to_string();
    let mut ftp = entry("beta");
    ftp.protocol = "ftp".to_string();
    let protocols = [porthmos_core::ProtocolInfo {
        id: "sftp",
        display_name: "SFTP",
        form: porthmos_core::ConnectionForm::standard(22),
    }];

    let backend = TestBackend::new(70, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_connections_list(frame, frame.area(), &[sftp, ftp], 0, &HashSet::new(), None, &protocols))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = (0..8).map(|y| (0..70).map(|x| buffer[(x, y)].symbol()).collect()).collect();

    assert!(rows.iter().any(|row| row.contains("SFTP") && row.contains("alpha")), "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains("ftp ") && row.contains("beta")), "{rows:#?}");
}

#[test]
fn the_protocol_column_is_as_wide_as_the_longest_protocol_name() {
    let mut short = entry("alpha");
    short.protocol = "sftp".to_string();
    let mut long = entry("beta");
    long.protocol = "s3-compatible".to_string();

    let rows = render_rows(&[short, long], &HashSet::new(), None);

    let column = |name: &str| rows.iter().find_map(|row| row.find(&format!(" {name} ("))).unwrap();
    assert_eq!(column("alpha"), column("beta"));
}
