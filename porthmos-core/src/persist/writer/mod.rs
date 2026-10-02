use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::{
    sync::{Notify, oneshot},
    time::Instant,
};

use super::{next_version, write_versioned};
use crate::tasks::{Scope, Tasks};

type Render = Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>;

struct Job {
    render: Render,
    mode: u32,
    tag: u64,
    version: u64,
}

type Pending = HashMap<PathBuf, Job>;

pub struct WriteFailure {
    pub path: PathBuf,
    pub message: String,
    pub tag: u64,
}

fn execute(path: &Path, job: Job) -> Result<(), String> {
    let contents = (job.render)()?;
    write_versioned(path, &contents, job.mode, job.version).map_err(|err| format!("{err:#}"))
}
type LastWritten = HashMap<PathBuf, Instant>;

#[derive(Default)]
struct State {
    pending: Pending,
    running: bool,
    flushing: Vec<oneshot::Sender<()>>,
}

struct Inner {
    tasks: Tasks,
    interval: Duration,
    on_failure: Box<dyn Fn(WriteFailure) + Send + Sync>,
    state: Mutex<State>,
    wake: Notify,
    closed: AtomicBool,
    writes: AtomicUsize,
}

struct Handle {
    inner: Arc<Inner>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.inner.close();
    }
}

#[derive(Clone)]
pub struct Writer {
    handle: Arc<Handle>,
}

struct RunningMark(Arc<Inner>);

impl Drop for RunningMark {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.0.state);
            state.running = false;
            state.flushing.clear();
        }
        self.0.write_pending_now();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Writer {
    pub fn new(tasks: Tasks, interval: Duration, on_failure: impl Fn(WriteFailure) + Send + Sync + 'static) -> Self {
        let inner = Arc::new(Inner {
            tasks,
            interval,
            on_failure: Box::new(on_failure),
            state: Mutex::new(State::default()),
            wake: Notify::new(),
            closed: AtomicBool::new(false),
            writes: AtomicUsize::new(0),
        });
        Self { handle: Arc::new(Handle { inner }) }
    }

    pub fn write(&self, path: PathBuf, contents: Vec<u8>, mode: u32) -> Result<(), String> {
        self.handle.inner.write_lazy(path, mode, 0, Box::new(move || Ok(contents)))
    }

    pub fn write_lazy(
        &self, path: PathBuf, mode: u32, tag: u64, render: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static,
    ) -> Result<(), String> {
        self.handle.inner.write_lazy(path, mode, tag, Box::new(render))
    }

    pub async fn flush(&self) {
        self.handle.inner.flush().await;
    }

    pub fn close(&self) {
        self.handle.inner.close();
    }

    pub fn write_pending_now(&self) {
        self.handle.inner.write_pending_now();
    }

    #[cfg(test)]
    pub(crate) fn writes_done(&self) -> usize {
        self.handle.inner.writes.load(Ordering::Relaxed)
    }
}

impl Inner {
    fn write_lazy(self: &Arc<Self>, path: PathBuf, mode: u32, tag: u64, render: Render) -> Result<(), String> {
        let job = Job { render, mode, tag, version: next_version() };
        let mut state = lock(&self.state);
        let queued =
            !self.closed.load(Ordering::Relaxed) && (state.running || tokio::runtime::Handle::try_current().is_ok());
        if !queued {
            state.pending.remove(&path);
            drop(state);
            self.writes.fetch_add(1, Ordering::Relaxed);
            return execute(&path, job);
        }
        let start_task = !state.running;
        state.running = true;
        state.pending.insert(path, job);
        drop(state);
        if start_task {
            self.start_task();
        }
        self.wake.notify_one();
        Ok(())
    }

    fn start_task(self: &Arc<Self>) {
        let mark = RunningMark(self.clone());
        let inner = self.clone();
        self.tasks.spawn("persist-writer", Scope::Background, move |_| async move {
            let _mark = mark;
            run(inner).await;
        });
    }

    async fn flush(&self) {
        let waiting = {
            let mut state = lock(&self.state);
            if state.running {
                let (done, finished) = oneshot::channel();
                state.flushing.push(done);
                Some(finished)
            } else {
                None
            }
        };
        match waiting {
            Some(finished) => {
                self.wake.notify_one();
                let _ = finished.await;
                if !lock(&self.state).running {
                    self.write_pending_now();
                }
            }
            None => self.write_pending_now(),
        }
    }

    fn close(&self) {
        {
            let _state = lock(&self.state);
            self.closed.store(true, Ordering::Relaxed);
        }
        self.wake.notify_one();
        self.write_pending_now();
    }

    fn write_pending_now(&self) {
        let pending = std::mem::take(&mut lock(&self.state).pending);
        for (path, job) in pending {
            let tag = job.tag;
            self.writes.fetch_add(1, Ordering::Relaxed);
            if let Err(message) = execute(&path, job) {
                (self.on_failure)(WriteFailure { path, message, tag });
            }
        }
    }
}

async fn run(inner: Arc<Inner>) {
    let mut last_written: LastWritten = HashMap::new();
    loop {
        let (due, closed, flushing) = {
            let mut state = lock(&inner.state);
            (
                next_due(&state.pending, &last_written, inner.interval),
                inner.closed.load(Ordering::Relaxed),
                std::mem::take(&mut state.flushing),
            )
        };
        if closed || !flushing.is_empty() {
            write_all(&inner, &mut last_written).await;
            for done in flushing {
                let _ = done.send(());
            }
            if closed {
                return;
            }
            continue;
        }
        tokio::select! {
            () = inner.wake.notified() => {}
            () = sleep_until_or_pending(due) => write_due(&inner, &mut last_written).await,
        }
    }
}

fn next_due(pending: &Pending, last_written: &LastWritten, interval: Duration) -> Option<Instant> {
    pending.keys().map(|path| last_written.get(path).map_or_else(Instant::now, |at| *at + interval)).min()
}

async fn sleep_until_or_pending(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn write_due(inner: &Arc<Inner>, last_written: &mut LastWritten) {
    let now = Instant::now();
    let due: Vec<(PathBuf, Job)> = {
        let mut state = lock(&inner.state);
        let paths: Vec<PathBuf> = state
            .pending
            .keys()
            .filter(|path| last_written.get(*path).is_none_or(|at| *at + inner.interval <= now))
            .cloned()
            .collect();
        paths.into_iter().filter_map(|path| state.pending.remove(&path).map(|job| (path, job))).collect()
    };
    for (path, job) in due {
        last_written.insert(path.clone(), Instant::now());
        write_one(inner, path, job).await;
    }
}

async fn write_all(inner: &Arc<Inner>, last_written: &mut LastWritten) {
    let all: Vec<(PathBuf, Job)> = std::mem::take(&mut lock(&inner.state).pending).into_iter().collect();
    for (path, job) in all {
        last_written.insert(path.clone(), Instant::now());
        write_one(inner, path, job).await;
    }
}

async fn write_one(inner: &Arc<Inner>, path: PathBuf, job: Job) {
    let tag = job.tag;
    let target = path.clone();
    let outcome = tokio::task::spawn_blocking(move || execute(&target, job)).await;
    inner.writes.fetch_add(1, Ordering::Relaxed);
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(message)) => (inner.on_failure)(WriteFailure { path, message, tag }),
        Err(join) => (inner.on_failure)(WriteFailure { path, message: join.to_string(), tag }),
    }
}

#[cfg(test)]
mod tests;
