use std::sync::Arc;

use async_trait::async_trait;

use super::FakeFs;
use crate::{
    Answer, ConnectionForm, Environment, ErrorKind, FileSystem, Prompter, Protocol, ProtocolError, Question,
    ShellInvocation, Target,
};

pub struct FakeProtocol {
    id: &'static str,
    form: Option<ConnectionForm>,
    fs: FakeFs,
    password: Option<String>,
    connect_failure: Option<String>,
    shell: Option<ShellInvocation>,
}

impl FakeProtocol {
    pub fn new(fs: FakeFs) -> Self {
        Self { id: "fake", form: None, fs, password: None, connect_failure: None, shell: None }
    }

    pub fn requiring_password(mut self, password: &str) -> Self {
        self.password = Some(password.to_string());
        self
    }

    pub fn failing_connect(mut self, message: &str) -> Self {
        self.connect_failure = Some(message.to_string());
        self
    }

    pub fn with_id(mut self, id: &'static str) -> Self {
        self.id = id;
        self
    }

    pub fn with_form(mut self, form: ConnectionForm) -> Self {
        self.form = Some(form);
        self
    }

    pub fn with_shell(mut self, invocation: ShellInvocation) -> Self {
        self.shell = Some(invocation);
        self
    }
}

#[async_trait]
impl Protocol for FakeProtocol {
    fn id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        "FAKE"
    }

    fn default_port(&self) -> u16 {
        2222
    }

    fn connection_form(&self) -> ConnectionForm {
        self.form.clone().unwrap_or_else(|| ConnectionForm::standard(self.default_port()))
    }

    async fn connect(
        &self, target: &Target, prompter: &mut dyn Prompter,
    ) -> Result<Arc<dyn FileSystem>, ProtocolError> {
        if let Some(message) = &self.connect_failure {
            return Err(ProtocolError::new(ErrorKind::Connect, anyhow::anyhow!(message.clone())));
        }
        if let Some(expected) = &self.password
            && target.password.as_ref() != Some(expected)
        {
            let question = Question::Password { username: target.username.clone(), name: target.name.clone() };
            match prompter.ask(question).await {
                None => {
                    return Err(ProtocolError::new(ErrorKind::Cancelled, anyhow::anyhow!("Connection cancelled")));
                }
                Some(Answer::Password(given)) if &given == expected => {}
                Some(_) => {
                    return Err(ProtocolError::new(ErrorKind::AuthRejected, anyhow::anyhow!("rejected")));
                }
            }
        }
        Ok(Arc::new(self.fs.clone()))
    }

    fn shell_command(&self, _target: &Target, _env: &Environment) -> Option<ShellInvocation> {
        self.shell.clone()
    }
}
