use std::fmt;

use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    Password { username: String, name: String },
}

#[derive(Clone, PartialEq, Eq)]
pub enum Answer {
    Password(String),
}

impl fmt::Debug for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Answer::Password(_) => f.write_str("Password(<redacted>)"),
        }
    }
}

#[async_trait]
pub trait Prompter: Send {
    async fn ask(&mut self, question: Question) -> Option<Answer>;
}

#[cfg(test)]
mod tests;
