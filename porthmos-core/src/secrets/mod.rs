use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use zeroize::Zeroizing;

pub const SERVICE: &str = "porthmos";
const PROBE_ACCOUNT: &str = "porthmos:probe";
const CALL_LIMIT: Duration = Duration::from_secs(30);
const WAITING_AFTER: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretField {
    Password,
    Option(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretKey {
    Profile { name: String, field: SecretField },
    SshHost { alias: String },
}

impl SecretKey {
    pub fn account(&self) -> String {
        match self {
            SecretKey::Profile { name, field: SecretField::Password } => format!("profile:{name}"),
            SecretKey::Profile { name, field: SecretField::Option(key) } => format!("option:{key}:{name}"),
            SecretKey::SshHost { alias } => format!("ssh:{alias}"),
        }
    }

    pub fn parse(account: &str) -> Option<Self> {
        if let Some(name) = account.strip_prefix("profile:") {
            return Some(SecretKey::Profile { name: name.to_string(), field: SecretField::Password });
        }
        if let Some(rest) = account.strip_prefix("option:") {
            let (key, name) = rest.split_once(':')?;
            return Some(SecretKey::Profile { name: name.to_string(), field: SecretField::Option(key.to_string()) });
        }
        account.strip_prefix("ssh:").map(|alias| SecretKey::SshHost { alias: alias.to_string() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretError {
    TimedOut,
    Backend(String),
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretError::TimedOut => write!(f, "timed out"),
            SecretError::Backend(message) => write!(f, "{message}"),
        }
    }
}

pub trait SecretBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError>;
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}

type WaitingReport = Arc<dyn Fn(bool) + Send + Sync>;

struct Inner {
    cache: Mutex<HashMap<String, Zeroizing<String>>>,
    backend: Option<Arc<dyn SecretBackend>>,
    available: AtomicBool,
    limit: Duration,
    waiting_after: Duration,
    waiting: Mutex<Option<WaitingReport>>,
    waiting_calls: AtomicUsize,
    backend_turn: Arc<Mutex<()>>,
}

#[derive(Clone)]
pub struct Secrets {
    inner: Arc<Inner>,
}

impl Secrets {
    pub fn new(backend: Option<Arc<dyn SecretBackend>>) -> Self {
        let available = AtomicBool::new(backend.is_some());
        Self {
            inner: Arc::new(Inner {
                cache: Mutex::new(HashMap::new()),
                backend,
                available,
                limit: CALL_LIMIT,
                waiting_after: WAITING_AFTER,
                waiting: Mutex::new(None),
                waiting_calls: AtomicUsize::new(0),
                backend_turn: Arc::new(Mutex::new(())),
            }),
        }
    }

    pub fn native() -> Self {
        Self::new(native_backend())
    }

    pub fn with_timing(self, limit: Duration, waiting_after: Duration) -> Self {
        let inner = Arc::try_unwrap(self.inner).unwrap_or_else(|_| panic!("with_timing on a shared Secrets"));
        Self { inner: Arc::new(Inner { limit, waiting_after, ..inner }) }
    }

    pub fn on_waiting(&self, report: impl Fn(bool) + Send + Sync + 'static) {
        *lock(&self.inner.waiting) = Some(Arc::new(report));
    }

    pub fn available(&self) -> bool {
        self.inner.available.load(Ordering::Relaxed)
    }

    pub async fn probe(&self) -> bool {
        let working = self.inner.backend.is_some() && self.call(|backend| backend.get(PROBE_ACCOUNT)).await.is_ok();
        self.inner.available.store(working, Ordering::Relaxed);
        working
    }

    pub fn cached(&self, account: &str) -> Option<Zeroizing<String>> {
        lock(&self.inner.cache).get(account).cloned()
    }

    pub async fn lookup(&self, account: &str, saved: bool) -> Option<Zeroizing<String>> {
        if let Some(hit) = self.cached(account) {
            return Some(hit);
        }
        if !saved || !self.available() {
            return None;
        }
        let owned = account.to_string();
        match self.call(move |backend| backend.get(&owned)).await {
            Ok(Some(secret)) => {
                let secret = Zeroizing::new(secret);
                lock(&self.inner.cache).insert(account.to_string(), secret.clone());
                Some(secret)
            }
            Ok(None) => None,
            Err(err) => {
                tracing::warn!("reading {account} from the system keyring failed: {err}");
                None
            }
        }
    }

    pub fn cached_accounts(&self) -> Vec<String> {
        lock(&self.inner.cache).keys().cloned().collect()
    }

    pub fn move_cached(&self, from: &str, to: &str) {
        let mut cache = lock(&self.inner.cache);
        if let Some(secret) = cache.remove(from) {
            cache.insert(to.to_string(), secret);
        }
    }

    pub fn uncache(&self, account: &str) {
        lock(&self.inner.cache).remove(account);
    }

    pub fn remember(&self, account: &str, secret: &str) {
        lock(&self.inner.cache).insert(account.to_string(), Zeroizing::new(secret.to_string()));
    }

    pub async fn save(&self, account: &str, secret: &str) -> Result<bool, SecretError> {
        self.remember(account, secret);
        self.write_keyring(account, secret).await
    }

    pub async fn forget(&self, account: &str, saved: bool) -> Result<(), SecretError> {
        self.uncache(account);
        if !saved {
            return Ok(());
        }
        self.erase_keyring(account).await
    }

    pub async fn rename(&self, from: &str, to: &str, saved: bool) -> Result<(), SecretError> {
        self.move_cached(from, to);
        if !saved {
            return Ok(());
        }
        self.move_keyring(from, to).await
    }

    pub async fn write_keyring(&self, account: &str, secret: &str) -> Result<bool, SecretError> {
        if !self.available() {
            return Ok(false);
        }
        let (owned, value) = (account.to_string(), Zeroizing::new(secret.to_string()));
        self.call(move |backend| backend.set(&owned, &value)).await.map(|()| true)
    }

    pub async fn erase_keyring(&self, account: &str) -> Result<(), SecretError> {
        if !self.available() {
            return Ok(());
        }
        let owned = account.to_string();
        self.call(move |backend| backend.delete(&owned)).await
    }

    pub async fn move_keyring(&self, from: &str, to: &str) -> Result<(), SecretError> {
        if !self.available() {
            return Ok(());
        }
        let (from, to) = (from.to_string(), to.to_string());
        self.call(move |backend| {
            if let Some(secret) = backend.get(&from)?.map(Zeroizing::new) {
                backend.set(&to, &secret)?;
                backend.delete(&from)?;
            }
            Ok(())
        })
        .await
    }

    async fn call<T: Send + 'static>(
        &self, work: impl FnOnce(&dyn SecretBackend) -> Result<T, SecretError> + Send + 'static,
    ) -> Result<T, SecretError> {
        let Some(backend) = self.inner.backend.clone() else {
            return Err(SecretError::Backend("no system keyring".into()));
        };
        let turn = self.inner.backend_turn.clone();
        let task = tokio::task::spawn_blocking(move || {
            let _turn = lock(&turn);
            work(backend.as_ref())
        });
        let timed = tokio::time::timeout(self.inner.limit, task);
        tokio::pin!(timed);
        let outcome = tokio::select! {
            outcome = &mut timed => outcome,
            () = tokio::time::sleep(self.inner.waiting_after) => {
                self.start_waiting();
                let outcome = timed.await;
                self.stop_waiting();
                outcome
            }
        };
        match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(join)) => Err(SecretError::Backend(join.to_string())),
            Err(_) => Err(SecretError::TimedOut),
        }
    }
}

impl Secrets {
    fn start_waiting(&self) {
        if self.inner.waiting_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.report_waiting(true);
        }
    }

    fn stop_waiting(&self) {
        if self.inner.waiting_calls.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.report_waiting(false);
        }
    }

    fn report_waiting(&self, waiting: bool) {
        let report = lock(&self.inner.waiting).clone();
        if let Some(report) = report {
            report(waiting);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(feature = "keyring")]
mod native;

#[cfg(feature = "keyring")]
fn native_backend() -> Option<Arc<dyn SecretBackend>> {
    Some(Arc::new(native::KeyringBackend::lazy()))
}

#[cfg(not(feature = "keyring"))]
fn native_backend() -> Option<Arc<dyn SecretBackend>> {
    None
}

#[cfg(any(test, feature = "testing"))]
mod testing;
#[cfg(any(test, feature = "testing"))]
pub use testing::TestBackend;

#[cfg(test)]
mod tests;
