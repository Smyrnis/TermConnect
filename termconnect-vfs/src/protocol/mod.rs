use std::{collections::BTreeMap, ffi::OsString, fmt, path::PathBuf, sync::Arc};

use async_trait::async_trait;

use crate::{FileSystem, Prompter, ProtocolError};

#[derive(Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub options: BTreeMap<String, String>,
}

impl Target {
    pub fn option(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(String::as_str)
    }
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Target")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("options", &self.options)
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    pub home: Option<PathBuf>,
    pub user: Option<String>,
    pub path: Option<OsString>,
}

impl Environment {
    pub fn from_process() -> Self {
        Self {
            home: std::env::var_os("HOME").map(PathBuf::from),
            user: std::env::var("USER").ok(),
            path: std::env::var_os("PATH"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ShellInvocation {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl ShellInvocation {
    pub fn to_command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command.args(&self.args);
        command.envs(self.env.iter().map(|(key, value)| (key, value)));
        command
    }
}

impl fmt::Debug for ShellInvocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted_env: Vec<(&OsString, &str)> = self.env.iter().map(|(key, _)| (key, "<redacted>")).collect();
        f.debug_struct("ShellInvocation")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &redacted_env)
            .finish()
    }
}

#[async_trait]
pub trait Protocol: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn default_port(&self) -> u16;
    fn discover(&self, _env: &Environment) -> Result<Vec<Target>, ProtocolError> {
        Ok(Vec::new())
    }
    async fn connect(&self, target: &Target, prompter: &mut dyn Prompter)
    -> Result<Arc<dyn FileSystem>, ProtocolError>;
    fn shell_command(&self, _target: &Target, _env: &Environment) -> Option<ShellInvocation> {
        None
    }
}

#[cfg(test)]
mod tests;
