use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::*;
use crate::engine::testing::capture_logs;

fn reporter() -> (Reporter, UnboundedReceiver<Event>) {
    let (events, received) = unbounded_channel();
    (Reporter::new(events), received)
}

fn notices(mut received: UnboundedReceiver<Event>) -> Vec<(Severity, String)> {
    std::iter::from_fn(|| received.try_recv().ok())
        .filter_map(|event| match event {
            Event::Notice { severity, message } => Some((severity, message)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_error_is_logged_at_error_level_and_shown_once() {
    let (reporter, received) = reporter();

    let logs = capture_logs(|| reporter.report(Severity::Error, "it broke"));

    assert!(logs.contains("ERROR") && logs.contains("it broke"), "{logs}");
    assert_eq!(notices(received), vec![(Severity::Error, "it broke".to_string())]);
}

#[test]
fn a_warning_is_logged_at_warn_level_and_shown_once() {
    let (reporter, received) = reporter();

    let logs = capture_logs(|| reporter.report(Severity::Warning, "careful"));

    assert!(logs.contains("WARN") && logs.contains("careful"), "{logs}");
    assert_eq!(notices(received), vec![(Severity::Warning, "careful".to_string())]);
}

#[test]
fn an_info_report_is_logged_at_info_level() {
    let (reporter, received) = reporter();

    let logs = capture_logs(|| reporter.report(Severity::Info, "fyi"));

    assert!(logs.contains("INFO") && logs.contains("fyi"), "{logs}");
    assert_eq!(notices(received).len(), 1);
}

#[test]
fn a_cause_is_logged_in_full_but_the_notice_carries_only_the_message() {
    let (reporter, received) = reporter();
    let cause = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "secret detail");

    let logs = capture_logs(|| reporter.report_cause(Severity::Error, "Unable to delete", &cause));

    assert!(logs.contains("Unable to delete"), "{logs}");
    assert!(logs.contains("secret detail"), "{logs}");
    assert_eq!(notices(received), vec![(Severity::Error, "Unable to delete".to_string())]);
}

#[test]
fn logging_only_writes_the_log_and_shows_nothing() {
    let (reporter, received) = reporter();

    let logs = capture_logs(|| reporter.log(Severity::Error, "already shown elsewhere"));

    assert!(logs.contains("ERROR") && logs.contains("already shown elsewhere"), "{logs}");
    assert!(notices(received).is_empty());
}

#[tokio::test]
async fn a_clone_reports_from_another_task() {
    let (reporter, mut received) = reporter();
    let clone = reporter.clone();

    tokio::spawn(async move { clone.report(Severity::Error, "from a task") }).await.unwrap();

    match received.recv().await {
        Some(Event::Notice { message, .. }) => assert_eq!(message, "from a task"),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn reporting_after_the_ui_is_gone_does_not_panic() {
    let (reporter, received) = reporter();
    drop(received);

    reporter.report(Severity::Error, "nobody is listening");
}

#[test]
fn showing_a_notice_writes_no_log() {
    let (reporter, received) = reporter();

    let logs = capture_logs(|| reporter.show(Severity::Error, "logged elsewhere"));

    assert!(logs.is_empty(), "{logs}");
    assert_eq!(notices(received), vec![(Severity::Error, "logged elsewhere".to_string())]);
}

#[test]
fn a_log_line_names_the_code_that_reported_it() {
    let (reporter, _received) = reporter();

    let logs = capture_logs(|| reporter.report(Severity::Error, "from a test"));

    assert!(logs.contains("report/tests.rs"), "{logs}");
}

fn rust_sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, found);
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".rs") && name != "tests.rs" && name != "testing.rs" {
            found.push(path);
        }
    }
}

fn squeezed(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap().chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn the_old_notice_helper_is_gone() {
    let mut files = Vec::new();
    rust_sources(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);

    let mut offenders = Vec::new();
    for path in files {
        let text = squeezed(&path);
        if text.contains(".notice(") {
            offenders.push(path.display().to_string());
        }
    }

    assert!(offenders.is_empty(), "these files still use the old notice helper: {offenders:?}");
}

#[test]
fn notice_events_are_only_built_in_the_reporter_and_the_info_helper() {
    let mut files = Vec::new();
    rust_sources(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);

    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut counts = Vec::new();
    for path in files {
        let text = squeezed(&path);
        let built = text.matches("Event::Notice{").count() + text.matches("useEvent::Notice").count();
        if built > 0 {
            counts.push((path.strip_prefix(&source).unwrap().display().to_string(), built));
        }
    }
    counts.sort();

    assert_eq!(counts, vec![("engine/mod.rs".to_string(), 1), ("engine/report/mod.rs".to_string(), 1)], "{counts:?}");
}
