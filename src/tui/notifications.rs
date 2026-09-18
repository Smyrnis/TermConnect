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
        self.queue.push_back(Notification { severity, message: message.into(), expires_at });
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
#[path = "../../tests/tui/notifications_test.rs"]
mod tests;
