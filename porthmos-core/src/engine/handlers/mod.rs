mod bookmarks;
mod connect;
mod edit;
mod fs;
mod history;
mod profiles;
mod search;
mod secrets;
mod transfer;

use std::fmt::Display;

pub(crate) use edit::{EditEvent, EditState};
pub(crate) use history::HistoryLog;
pub(crate) use profiles::{KeyringDone, KeyringJob};

use super::Location;
use crate::user_message;

fn failure_message(location: Location, context: impl AsRef<str>, err: &dyn Display) -> String {
    match location {
        Location::Local => err.to_string(),
        Location::Session(_) => user_message(context, err),
    }
}
