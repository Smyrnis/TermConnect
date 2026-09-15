use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    /// `None` means "never auto-expires" — only `Esc` dismisses it.
    fn ttl(self) -> Option<Duration> {
        match self {
            Severity::Info => Some(Duration::from_secs(4)),
            Severity::Warning => Some(Duration::from_secs(8)),
            Severity::Error => None,
        }
    }
}

pub struct Notification {
    pub severity: Severity,
    pub message: String,
    expires_at: Option<Instant>,
}

/// A FIFO queue of status-bar messages. An `Error` at the front blocks
/// later messages from showing until it's dismissed — errors need
/// acknowledgment, so they're never silently superseded.
#[derive(Default)]
pub struct Notifications {
    queue: VecDeque<Notification>,
}

impl Notifications {
    pub fn push(&mut self, severity: Severity, message: impl Into<String>) {
        let expires_at = severity.ttl().map(|ttl| Instant::now() + ttl);
        self.queue.push_back(Notification {
            severity,
            message: message.into(),
            expires_at,
        });
    }

    pub fn current(&self) -> Option<&Notification> {
        self.queue.front()
    }

    pub fn dismiss_current(&mut self) {
        self.queue.pop_front();
    }

    /// Drops leading entries whose deadline has passed. Only ever looks at
    /// the front — a later entry can't be showing yet, so its expiry
    /// doesn't matter until it reaches the front.
    pub fn expire(&mut self, now: Instant) {
        while let Some(front) = self.queue.front() {
            match front.expires_at {
                Some(at) if at <= now => {
                    self.queue.pop_front();
                }
                _ => break,
            }
        }
    }

    /// When the event loop should next re-check `expire`, or `None` if
    /// there's nothing to wait for (empty queue, or the front entry never
    /// expires on its own).
    pub fn next_wake(&self) -> Option<Instant> {
        self.queue.front().and_then(|n| n.expires_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
