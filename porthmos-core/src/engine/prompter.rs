use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use porthmos_vfs::{Answer, Prompter, Question, async_trait};
use tokio::sync::{mpsc::UnboundedSender, oneshot};
use zeroize::Zeroizing;

use super::{Event, RequestId};

type Reply = oneshot::Sender<Option<(Answer, bool)>>;

#[derive(Clone, Default)]
pub(crate) struct PendingQuestions {
    waiting: Arc<Mutex<HashMap<RequestId, Reply>>>,
    next_id: Arc<AtomicU64>,
}

impl PendingQuestions {
    pub(crate) fn answer(&self, request_id: RequestId, answer: Option<Answer>, save: bool) {
        let waiting = self.waiting.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&request_id);
        if let Some(reply) = waiting {
            let _ = reply.send(answer.map(|answer| (answer, save)));
        }
    }

    fn register(&self) -> (RequestId, oneshot::Receiver<Option<(Answer, bool)>>) {
        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply, receiver) = oneshot::channel();
        self.waiting.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(request_id, reply);
        (request_id, receiver)
    }
}

pub(crate) struct TypedPassword {
    pub(crate) secret: Zeroizing<String>,
    pub(crate) save: bool,
}

pub(crate) struct EnginePrompter {
    pub(crate) questions: PendingQuestions,
    pub(crate) events: UnboundedSender<Event>,
    pub(crate) typed: Option<TypedPassword>,
}

#[async_trait]
impl Prompter for EnginePrompter {
    async fn ask(&mut self, question: Question) -> Option<Answer> {
        let asks_for_password = matches!(question, Question::Password { .. });
        let (request_id, receiver) = self.questions.register();
        self.events.send(Event::Question { request_id, question }).ok()?;
        let (answer, save) = receiver.await.ok().flatten()?;
        if asks_for_password && let Answer::Password(secret) = &answer {
            self.typed = Some(TypedPassword { secret: Zeroizing::new(secret.clone()), save });
        }
        Some(answer)
    }
}
