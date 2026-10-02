use porthmos_vfs::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Security {
    Plain,
    Explicit,
    Implicit,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FtpSettings {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) password: Option<String>,
    pub(crate) security: Security,
    pub(crate) passive: bool,
}

const ANONYMOUS: &str = "anonymous";
const ANONYMOUS_PASSWORD: &str = "anonymous@";

impl FtpSettings {
    pub(crate) fn from_target(target: &Target) -> Self {
        let anonymous = target.username.trim().is_empty();
        Self {
            host: target.host.clone(),
            port: target.port,
            username: if anonymous { ANONYMOUS.to_string() } else { target.username.clone() },
            password: match (&target.password, anonymous) {
                (Some(password), _) => Some(password.clone()),
                (None, true) => Some(ANONYMOUS_PASSWORD.to_string()),
                (None, false) => None,
            },
            security: match target.option("security") {
                Some("plain") => Security::Plain,
                Some("implicit") => Security::Implicit,
                _ => Security::Explicit,
            },
            passive: target.option("passive") != Some("false"),
        }
    }

    pub(crate) fn is_anonymous(&self) -> bool {
        self.username == ANONYMOUS
    }
}

#[cfg(test)]
mod tests;
