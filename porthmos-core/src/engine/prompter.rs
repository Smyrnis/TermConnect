use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use porthmos_vfs::{Answer, Prompter, Question, async_trait};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use super::{Event, RequestId};

#[derive(Clone, Default)]
pub(crate) struct PendingQuestions {
    waiting: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Option<Answer>>>>>,
    next_id: Arc<AtomicU64>,
}

impl PendingQuestions {
    pub(crate) fn answer(&self, request_id: RequestId, answer: Option<Answer>) {
        let waiting = self.waiting.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&request_id);
        if let Some(reply) = waiting {
            let _ = reply.send(answer);
        }
    }

    fn register(&self) -> (RequestId, oneshot::Receiver<Option<Answer>>) {
        let request_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply, receiver) = oneshot::channel();
        self.waiting.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(request_id, reply);
        (request_id, receiver)
    }
}

pub(crate) struct EnginePrompter {
    pub(crate) questions: PendingQuestions,
    pub(crate) events: UnboundedSender<Event>,
}

#[async_trait]
impl Prompter for EnginePrompter {
    async fn ask(&mut self, question: Question) -> Option<Answer> {
        let (request_id, receiver) = self.questions.register();
        self.events.send(Event::Question { request_id, question }).ok()?;
        receiver.await.ok().flatten()
    }
}
