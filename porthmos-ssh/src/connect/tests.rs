use anyhow::anyhow;
use porthmos_vfs::{Answer, ErrorKind, Prompter, Question, Target};

use super::*;
use crate::client;

struct Recording {
    answer: Option<Answer>,
    asked: Vec<Question>,
}

#[async_trait::async_trait]
impl Prompter for Recording {
    async fn ask(&mut self, question: Question) -> Option<Answer> {
        self.asked.push(question);
        self.answer.clone()
    }
}

fn target() -> Target {
    Target {
        name: "web".into(),
        host: "example.com".into(),
        port: 2222,
        username: "u".into(),
        password: None,
        options: Default::default(),
    }
}

async fn ask_once_while_connecting(prompter: &mut Recording) -> bool {
    let (questions, mut asked) = tokio::sync::mpsc::channel(1);
    let connecting = async move {
        let (reply, answer) = tokio::sync::oneshot::channel();
        let question =
            client::HostKeyQuestion { key_type: "ssh-ed25519".into(), fingerprint: "SHA256:abc".into(), reply };
        questions.send(question).await.unwrap();
        answer.await.unwrap()
    };
    answer_host_key_questions(connecting, &mut asked, prompter, &target()).await
}

#[tokio::test]
async fn a_host_key_question_is_asked_through_the_prompter_while_connecting() {
    let mut prompter = Recording { answer: Some(Answer::Confirmed), asked: Vec::new() };

    assert!(ask_once_while_connecting(&mut prompter).await);
    assert_eq!(
        prompter.asked,
        [Question::TrustHostKey {
            name: "web".into(),
            host: "example.com".into(),
            port: 2222,
            key_type: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
        }]
    );
}

#[tokio::test]
async fn cancelling_the_host_key_question_declines_the_key() {
    let mut prompter = Recording { answer: None, asked: Vec::new() };

    assert!(!ask_once_while_connecting(&mut prompter).await);
}

#[test]
fn a_declined_host_key_is_reported_as_a_cancelled_connection() {
    let error = connect_error(anyhow::Error::new(client::HostKeyDeclined));

    assert_eq!(error.kind(), ErrorKind::Cancelled);
    assert_eq!(error.to_string(), "Connection cancelled");
}

#[test]
fn any_other_handshake_failure_is_a_connect_error() {
    assert_eq!(connect_error(anyhow!("connection refused")).kind(), ErrorKind::Connect);
}
