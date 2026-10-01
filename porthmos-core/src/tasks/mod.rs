use std::{
    any::Any,
    cell::Cell,
    collections::{HashMap, HashSet},
    future::{Future, poll_fn},
    panic::AssertUnwindSafe,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::FutureExt;
use tokio::{sync::Notify, task::AbortHandle, time::Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Session(u64),
    Transfer(u64),
    Planning(u64),
    Edit(u64),
    Search,
    Background,
}

struct Live {
    name: &'static str,
    scope: Scope,
    cancel: Arc<AtomicBool>,
    abort: AbortHandle,
}

struct Shared {
    live: Mutex<HashMap<u64, Live>>,
    cancelled: Mutex<HashSet<Scope>>,
    next: AtomicU64,
    changed: Notify,
    on_panic: Box<dyn Fn(&'static str, Scope, String) + Send + Sync>,
}

impl Shared {
    fn finish(&self, id: u64) {
        lock(&self.live).remove(&id);
        self.changed.notify_waiters();
    }
}

#[derive(Clone)]
pub struct Tasks {
    shared: Arc<Shared>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

thread_local! {
    static SUPERVISED_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub fn in_supervised_task() -> bool {
    SUPERVISED_DEPTH.with(|depth| depth.get() > 0)
}

struct SupervisedPoll;

impl SupervisedPoll {
    fn enter() -> Self {
        SUPERVISED_DEPTH.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for SupervisedPoll {
    fn drop(&mut self) {
        SUPERVISED_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

fn supervised<Fut: Future<Output = ()>>(future: Fut) -> impl Future<Output = ()> {
    let mut future = Box::pin(future);
    poll_fn(move |context| {
        let _poll = SupervisedPoll::enter();
        future.as_mut().poll(context)
    })
}

fn panic_text(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "the task panicked".to_string()
    }
}

impl Tasks {
    pub fn new(on_panic: impl Fn(&'static str, Scope, String) + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                live: Mutex::new(HashMap::new()),
                cancelled: Mutex::new(HashSet::new()),
                next: AtomicU64::new(0),
                changed: Notify::new(),
                on_panic: Box::new(on_panic),
            }),
        }
    }

    pub fn spawn<F, Fut>(&self, name: &'static str, scope: Scope, work: F) -> Arc<AtomicBool>
    where
        F: FnOnce(Arc<AtomicBool>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let future = work(cancel.clone());
        let id = self.shared.next.fetch_add(1, Ordering::Relaxed);
        let shared = self.shared.clone();
        let mut live = lock(&self.shared.live);
        let handle = tokio::spawn(async move {
            let outcome = AssertUnwindSafe(supervised(future)).catch_unwind().await;
            shared.finish(id);
            if let Err(payload) = outcome {
                (shared.on_panic)(name, scope, panic_text(payload.as_ref()));
            }
        });
        live.insert(id, Live { name, scope, cancel: cancel.clone(), abort: handle.abort_handle() });
        cancel
    }

    pub fn cancel(&self, scope: Scope) {
        for live in lock(&self.shared.live).values().filter(|live| live.scope == scope) {
            live.cancel.store(true, Ordering::Relaxed);
        }
        lock(&self.shared.cancelled).insert(scope);
    }

    pub fn take_cancelled(&self, scope: Scope) -> bool {
        lock(&self.shared.cancelled).remove(&scope)
    }

    pub fn forget(&self, scope: Scope) {
        lock(&self.shared.cancelled).remove(&scope);
    }

    pub fn cancel_all(&self) {
        for live in lock(&self.shared.live).values() {
            live.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn live_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = lock(&self.shared.live).values().map(|live| live.name).collect();
        names.sort_unstable();
        names
    }

    pub async fn wait_for_name(&self, name: &'static str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.shared.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !lock(&self.shared.live).values().any(|live| live.name == name) {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || tokio::time::timeout(remaining, notified).await.is_err() {
                return false;
            }
        }
    }

    pub async fn shutdown(&self, grace: Duration) {
        let deadline = Instant::now() + grace;
        loop {
            let notified = self.shared.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if lock(&self.shared.live).is_empty() {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || tokio::time::timeout(remaining, notified).await.is_err() {
                break;
            }
        }
        self.abort_all();
    }

    pub fn abort_all(&self) {
        for (_, live) in lock(&self.shared.live).drain() {
            live.abort.abort();
        }
    }
}

#[cfg(test)]
mod tests;
