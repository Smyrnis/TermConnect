use super::*;
use std::time::{Duration, Instant};

#[test]
fn push_makes_the_message_current() {
    let mut notifications = Notifications::default();
    notifications.push(Severity::Info, "saved");
    assert_eq!(notifications.current().unwrap().message, "saved");
}

#[test]
fn earlier_pushes_stay_current_until_dismissed_fifo() {
    let mut notifications = Notifications::default();
    notifications.push(Severity::Info, "first");
    notifications.push(Severity::Info, "second");

    assert_eq!(notifications.current().unwrap().message, "first");
    notifications.dismiss_current();
    assert_eq!(notifications.current().unwrap().message, "second");
}

#[test]
fn expire_drops_only_notifications_whose_deadline_has_passed() {
    let now = Instant::now();
    let mut notifications = Notifications::default();
    notifications.queue.push_back(Notification {
        severity: Severity::Info,
        message: "old".to_string(),
        expires_at: Some(now),
    });
    notifications.queue.push_back(Notification {
        severity: Severity::Info,
        message: "fresh".to_string(),
        expires_at: Some(now + Duration::from_secs(10)),
    });

    notifications.expire(now + Duration::from_millis(1));

    assert_eq!(notifications.current().unwrap().message, "fresh");
}

#[test]
fn errors_never_auto_expire() {
    let now = Instant::now();
    let mut notifications = Notifications::default();
    notifications.push(Severity::Error, "connection failed");

    notifications.expire(now + Duration::from_secs(3600));

    assert_eq!(
        notifications.current().unwrap().message,
        "connection failed"
    );
}

#[test]
fn next_wake_is_none_when_the_queue_is_empty_or_fronted_by_an_error() {
    let mut notifications = Notifications::default();
    assert_eq!(notifications.next_wake(), None);

    notifications.push(Severity::Error, "boom");
    assert_eq!(notifications.next_wake(), None);
}

#[test]
fn next_wake_returns_the_front_entrys_deadline() {
    let mut notifications = Notifications::default();
    notifications.push(Severity::Warning, "careful");
    assert!(notifications.next_wake().is_some());
}
