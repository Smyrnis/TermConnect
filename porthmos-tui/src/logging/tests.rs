use std::{
    fs,
    io::Write,
    sync::{Arc, Mutex},
};

use tracing_subscriber::{EnvFilter, prelude::*};

use super::*;

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

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
    type Writer = Buffer;

    fn make_writer(&'a self) -> Buffer {
        self.clone()
    }
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
