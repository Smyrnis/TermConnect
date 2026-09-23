use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
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

    pub fn next_wake(&self) -> Option<Instant> {
        self.queue.front().and_then(|n| n.expires_at)
    }
}

#[cfg(test)]
#[path = "../../tests/tui/notifications_test.rs"]
mod tests;
