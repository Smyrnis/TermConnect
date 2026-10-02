#[cfg(not(unix))]
compile_error!("porthmos only supports Unix-like platforms (Linux/macOS)");

mod fs;
mod search;
mod subsystem;

use std::sync::Arc;

use porthmos_ssh::{ConnectOptions, DEFAULT_PORT, Session};
use porthmos_vfs::{
    ConnectionForm, Environment, ErrorKind, FileSystem, OptionField, OptionKind, Prompter, Protocol, ProtocolError,
    ShellInvocation, Target, async_trait,
};

use crate::fs::SftpFs;

pub struct Sftp;

async fn start_sftp(session: Session) -> Result<Arc<dyn FileSystem>, ProtocolError> {
    let sftp = subsystem::open_sftp(&session).await.map_err(|err| ProtocolError::new(ErrorKind::SessionStart, err))?;
    Ok(Arc::new(SftpFs::new(Arc::new(session), Arc::new(sftp))))
}

#[async_trait]
impl Protocol for Sftp {
    fn id(&self) -> &'static str {
        "sftp"
    }

    fn display_name(&self) -> &'static str {
        "SFTP"
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

    fn discover(&self, env: &Environment) -> Result<Vec<Target>, ProtocolError> {
        porthmos_ssh::discover(env)
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        let session = porthmos_ssh::connect(target, prompter, &ConnectOptions::default()).await?;
        start_sftp(session).await
    }

    fn shell_command(&self, target: &Target, env: &Environment) -> Option<ShellInvocation> {
        Some(porthmos_ssh::shell_command(target, env))
    }
}

#[cfg(test)]
mod tests;
