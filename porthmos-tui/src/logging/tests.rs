use std::{fs, io::Write};

use tracing_subscriber::{EnvFilter, prelude::*};

use super::{testing::Buffer, *};

#[test]
fn open_writer_at_creates_parent_directories_and_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("porthmos.log");

    open_writer_at(&path).unwrap();

    assert!(path.exists());
}

#[test]
fn open_writer_at_appends_rather_than_truncating_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("porthmos.log");
    {
        let mut file = open_writer_at(&path).unwrap();
        writeln!(file, "first").unwrap();
    }
    {
        let mut file = open_writer_at(&path).unwrap();
        writeln!(file, "second").unwrap();
    }

    let contents = fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "first\nsecond\n");
}

fn logged_with(base: &str) -> String {
    let buffer = Buffer::default();
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(buffer.clone())
        .with_ansi(false)
        .with_filter(with_transfer_log(EnvFilter::new(base)));
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(target: "porthmos::transfers", "transfer reason");
        tracing::error!(target: "porthmos::other", "unrelated error");
        tracing::info!(target: "porthmos::transfers", "transfer chatter");
    });
    let bytes = buffer.0.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn transfer_failures_are_logged_even_with_logging_switched_off() {
    let logs = logged_with("off");

    assert!(logs.contains("transfer reason"), "{logs}");
    assert!(!logs.contains("unrelated error"), "{logs}");
    assert!(!logs.contains("transfer chatter"), "{logs}");
}

#[test]
fn transfer_failures_are_logged_when_only_the_crate_is_silenced() {
    let logs = logged_with("porthmos=off");

    assert!(logs.contains("transfer reason"), "{logs}");
    assert!(!logs.contains("unrelated error"), "{logs}");
}

#[test]
fn transfer_failures_are_logged_when_the_transfer_target_itself_is_silenced() {
    let logs = logged_with("porthmos::transfers=off");

    assert!(logs.contains("transfer reason"), "{logs}");
}

#[test]
fn a_verbose_setting_is_left_alone() {
    let logs = logged_with("info");

    assert!(logs.contains("transfer reason"), "{logs}");
    assert!(logs.contains("unrelated error"), "{logs}");
    assert!(logs.contains("transfer chatter"), "{logs}");
}

fn logged_by_default(directives: Option<&str>) -> String {
    let buffer = Buffer::default();
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(buffer.clone())
        .with_ansi(false)
        .with_filter(log_filter(directives));
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(target: "porthmos_core::engine::report", "core warning");
        tracing::info!(target: "porthmos_core::engine::report", "core chatter");
        tracing::warn!(target: "porthmos_tui::app", "tui warning");
        tracing::warn!(target: "porthmos_sftp::session", "protocol warning");
        tracing::warn!(target: "porthmos_vfs", "vfs warning");
        tracing::info!(target: "porthmos_sftp::session", "protocol chatter");
        tracing::warn!(target: "hyper::client", "dependency warning");
        tracing::error!(target: "hyper::client", "dependency error");
    });
    let bytes = buffer.0.lock().unwrap().clone();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn without_a_setting_the_log_keeps_porthmos_warnings_and_every_error() {
    let logs = logged_by_default(None);

    assert!(logs.contains("core warning"), "{logs}");
    assert!(logs.contains("tui warning"), "{logs}");
    assert!(logs.contains("protocol warning"), "{logs}");
    assert!(logs.contains("vfs warning"), "{logs}");
    assert!(logs.contains("dependency error"), "{logs}");
    assert!(!logs.contains("core chatter"), "{logs}");
    assert!(!logs.contains("protocol chatter"), "{logs}");
    assert!(!logs.contains("dependency warning"), "{logs}");
}

#[test]
fn an_empty_setting_counts_as_no_setting() {
    let logs = logged_by_default(Some(""));

    assert!(logs.contains("core warning"), "{logs}");
}

#[test]
fn an_explicit_setting_replaces_the_default() {
    let logs = logged_by_default(Some("error"));

    assert!(!logs.contains("core warning"), "{logs}");
    assert!(logs.contains("dependency error"), "{logs}");
}
