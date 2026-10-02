use porthmos_vfs::Target;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct WebDavSettings {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) secure: bool,
    pub(crate) username: String,
    pub(crate) password: Option<String>,
    pub(crate) root: String,
}

impl WebDavSettings {
    pub(crate) fn from_target(target: &Target) -> Self {
        Self {
            host: target.host.trim().to_string(),
            port: target.port,
            secure: target.option("security") != Some("http"),
            username: target.username.trim().to_string(),
            password: target.password.clone().filter(|password| !password.is_empty()),
            root: normalize_root(target.option("root").unwrap_or("/")),
        }
    }

    pub(crate) fn origin(&self) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("{scheme}://{host}:{}", self.port)
    }
}

pub(crate) fn normalize_root(root: &str) -> String {
    let segments: Vec<&str> = root.split('/').map(str::trim).filter(|segment| !segment.is_empty()).collect();
    if segments.is_empty() { "/".to_string() } else { format!("/{}/", segments.join("/")) }
}

#[cfg(test)]
mod tests;
