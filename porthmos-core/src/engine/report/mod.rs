use std::fmt::Debug;

use tokio::sync::mpsc::UnboundedSender;

use super::Event;
use crate::Severity;

#[derive(Clone)]
pub(crate) struct Reporter {
    events: UnboundedSender<Event>,
}

impl Reporter {
    pub(crate) fn new(events: UnboundedSender<Event>) -> Self {
        Self { events }
    }

    #[track_caller]
    pub(crate) fn log(&self, severity: Severity, message: &str) {
        let at = std::panic::Location::caller();
        match severity {
            Severity::Error => tracing::error!(%at, "{message}"),
            Severity::Warning => tracing::warn!(%at, "{message}"),
            Severity::Info => tracing::info!(%at, "{message}"),
        }
    }

    pub(crate) fn show(&self, severity: Severity, message: impl Into<String>) {
        let _ = self.events.send(Event::Notice { severity, message: message.into() });
    }

    #[track_caller]
    pub(crate) fn report(&self, severity: Severity, message: impl Into<String>) {
        let message = message.into();
        self.log(severity, &message);
        self.show(severity, message);
    }

    #[track_caller]
    pub(crate) fn report_cause(&self, severity: Severity, message: impl Into<String>, cause: &dyn Debug) {
        tracing::debug!("{cause:?}");
        self.report(severity, message);
    }
}

#[cfg(test)]
mod tests;
