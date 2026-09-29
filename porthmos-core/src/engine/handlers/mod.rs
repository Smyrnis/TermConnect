mod bookmarks;
mod connect;
mod fs;
mod profiles;
mod search;
mod secrets;
mod transfer;

use std::fmt::Display;

pub(crate) use profiles::{KeyringDone, KeyringJob};

use super::Location;
use crate::user_message;

fn failure_message(location: Location, context: impl AsRef<str>, err: &dyn Display) -> String {
    match location {
        Location::Local => err.to_string(),
        Location::Session(_) => user_message(context, err),
    }
}
