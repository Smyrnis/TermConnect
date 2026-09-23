use anyhow::Error;

pub fn user_message(context: impl AsRef<str>, err: &Error) -> String {
    format!("{}:\n{err}", context.as_ref())
}

#[cfg(test)]
#[path = "../tests/root/errors_test.rs"]
mod tests;
