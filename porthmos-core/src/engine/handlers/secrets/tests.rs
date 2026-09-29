use std::{sync::Arc, time::Duration};

use crate::{
    Severity,
    engine::{Command, Event, testing::test_engine_with_secrets},
    secrets::TestBackend,
};

fn waiting(events: &[Event]) -> Vec<bool> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::KeyringWaiting { waiting } => Some(*waiting),
            _ => None,
        })
        .collect()
}

fn choices(events: &[Event]) -> Vec<bool> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::SaveChoice { save } => Some(*save),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn starting_probes_the_keyring_and_reports_it() {
    let backend = Arc::new(TestBackend::new());
    let mut t = test_engine_with_secrets(backend.clone());

    t.engine.start_keyring_probe();
    t.run_internal().await;

    let events = t.drain();
    assert!(matches!(events.as_slice(), [Event::KeyringStatus { available: true }]), "{events:?}");
    assert_eq!(backend.calls(), vec!["get porthmos:probe"]);
    assert!(t.engine.secrets.available());
}

#[tokio::test]
async fn a_failing_probe_reports_no_keyring() {
    let backend = Arc::new(TestBackend::new());
    backend.fail_reads();
    let mut t = test_engine_with_secrets(backend);

    t.engine.start_keyring_probe();
    t.run_internal().await;

    assert!(t.drain().iter().any(|event| matches!(event, Event::KeyringStatus { available: false })));
    assert!(!t.engine.secrets.available());
}

#[tokio::test]
async fn an_engine_without_a_keyring_reports_none_without_calling_anything() {
    let mut t = crate::engine::testing::test_engine();

    t.engine.start_keyring_probe();
    t.run_internal().await;

    assert!(t.drain().iter().any(|event| matches!(event, Event::KeyringStatus { available: false })));
}

#[tokio::test]
async fn a_slow_keyring_is_reported_as_waiting_and_then_done() {
    let backend = Arc::new(TestBackend::new());
    backend.block_for(Duration::from_millis(150));
    let mut t = test_engine_with_secrets(backend);

    t.engine.start_keyring_probe();
    t.run_internal().await;

    assert_eq!(waiting(&t.drain()), vec![true, false]);
}

#[tokio::test]
async fn a_fast_keyring_is_never_reported_as_waiting() {
    let backend = Arc::new(TestBackend::new());
    let mut t = test_engine_with_secrets(backend);

    t.engine.start_keyring_probe();
    t.run_internal().await;

    assert!(waiting(&t.drain()).is_empty());
}

#[test]
fn the_save_choice_is_published_and_remembered() {
    let backend = Arc::new(TestBackend::new());
    let mut t = test_engine_with_secrets(backend);

    t.engine.publish_save_choice();
    t.engine.handle_command(Command::RememberSaveChoice { save: true });
    t.engine.publish_save_choice();
    t.engine.handle_command(Command::RememberSaveChoice { save: false });
    t.engine.publish_save_choice();

    assert_eq!(choices(&t.drain()), vec![false, true, false]);
}

#[test]
fn a_choice_that_cannot_be_written_is_a_warning() {
    let backend = Arc::new(TestBackend::new());
    let mut t = test_engine_with_secrets(backend);
    std::fs::remove_dir_all(&t.engine.paths.state_dir).ok();
    std::fs::create_dir_all(t.engine.paths.state_dir.parent().unwrap()).unwrap();
    std::fs::write(&t.engine.paths.state_dir, b"a file").unwrap();

    t.engine.handle_command(Command::RememberSaveChoice { save: true });

    assert!(matches!(t.drain().as_slice(), [Event::Notice { severity: Severity::Warning, .. }]));
}
