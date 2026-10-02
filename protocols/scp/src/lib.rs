#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod commands;
mod fs;
mod listing;
mod streams;
mod wire;

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::anyhow;
use porthmos_ssh::{ConnectOptions, DEFAULT_PORT, exec};
use porthmos_vfs::{
    ConnectionForm, Environment, ErrorKind, FileSystem, OptionField, OptionKind, Prompter, Protocol, ProtocolError,
    ShellInvocation, Target, async_trait,
};

use crate::fs::ScpFs;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Scp {
    connect: ConnectOptions,
    timeout: Duration,
}

impl Default for Scp {
    fn default() -> Self {
        Self { connect: ConnectOptions::default(), timeout: DEFAULT_TIMEOUT }
    }
}

impl Scp {
    pub fn with_connect_options(mut self, options: ConnectOptions) -> Self {
        self.connect = options;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

fn no_shell() -> ProtocolError {
    ProtocolError::new(
        ErrorKind::SessionStart,
        anyhow!("the server does not allow running commands (SCP needs a shell)"),
    )
}

#[async_trait]
impl Protocol for Scp {
    fn id(&self) -> &'static str {
        "scp"
    }

    fn display_name(&self) -> &'static str {
        "SCP"
    }

    fn default_port(&self) -> u16 {
        DEFAULT_PORT
    }

    fn connection_form(&self) -> ConnectionForm {
        let mut form = ConnectionForm::standard(DEFAULT_PORT);
        form.options.push(OptionField {
            key: "identity_file",
            label: "Identity file",
            required: false,
            kind: OptionKind::Text { default: "" },
        });
        form
    }

    fn discover(&self, _env: &Environment) -> Result<Vec<Target>, ProtocolError> {
        Ok(Vec::new())
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        let session = porthmos_ssh::connect(target, prompter, &self.connect).await?;
        let probe = tokio::time::timeout(self.timeout, exec(&session, commands::PROBE))
            .await
            .map_err(|_| no_shell())?
            .map_err(|_| no_shell())?;
        let stdout = String::from_utf8_lossy(&probe.stdout).into_owned();
        let mut lines = stdout.lines();
        if probe.status != Some(0) || lines.next() != Some(commands::PROBE_MARKER) {
            return Err(no_shell());
        }
        let home = lines.next().map(str::trim).filter(|home| home.starts_with('/')).unwrap_or("/");
        let scp = lines.any(|line| line.trim() == "scp");
        Ok(Arc::new(ScpFs::new(Arc::new(session), PathBuf::from(home), scp, self.timeout)))
    }

    fn shell_command(&self, target: &Target, env: &Environment) -> Option<ShellInvocation> {
        Some(porthmos_ssh::shell_command(target, env))
    }
}

#[cfg(test)]
mod tests;
