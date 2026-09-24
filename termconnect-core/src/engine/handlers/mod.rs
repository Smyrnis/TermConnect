mod bookmarks;
mod connect;
mod fs;
mod profiles;
mod search;
mod transfer;

use std::fmt::Display;

use super::Location;
use crate::user_message;

fn failure_message(location: Location, context: impl AsRef<str>, err: &dyn Display) -> String {
    match location {
        Location::Local => err.to_string(),
        Location::Session(_) => user_message(context, err),
    }
}
