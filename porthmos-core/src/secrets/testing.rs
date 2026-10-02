use std::{collections::HashMap, sync::Mutex, time::Duration};

use super::{SecretBackend, SecretError, lock};

#[derive(Default)]
pub struct TestBackend {
    stored: Mutex<HashMap<String, String>>,
    calls: Mutex<Vec<String>>,
    fail_reads: Mutex<bool>,
    fail_writes: Mutex<bool>,
    block: Mutex<Option<Duration>>,
    block_once: Mutex<Option<Duration>>,
}

impl TestBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&self, account: &str, secret: &str) {
        lock(&self.stored).insert(account.to_string(), secret.to_string());
    }

    pub fn stored(&self, account: &str) -> Option<String> {
        lock(&self.stored).get(account).cloned()
    }

    pub fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    pub fn fail_reads(&self) {
        *lock(&self.fail_reads) = true;
    }

    pub fn fail_writes(&self) {
        *lock(&self.fail_writes) = true;
    }

    pub fn block_for(&self, pause: Duration) {
        *lock(&self.block) = Some(pause);
    }

    pub fn block_next(&self, pause: Duration) {
        *lock(&self.block_once) = Some(pause);
    }

    fn enter(&self, call: String) {
        lock(&self.calls).push(call);
        let once = lock(&self.block_once).take();
        if let Some(pause) = once.or(*lock(&self.block)) {
            std::thread::sleep(pause);
        }
    }
}

impl SecretBackend for TestBackend {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        self.enter(format!("get {account}"));
        if *lock(&self.fail_reads) {
            return Err(SecretError::Backend("read failed".into()));
        }
        Ok(self.stored(account))
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.enter(format!("set {account}"));
        if *lock(&self.fail_writes) {
            return Err(SecretError::Backend("write failed".into()));
        }
        self.put(account, secret);
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.enter(format!("delete {account}"));
        if *lock(&self.fail_writes) {
            return Err(SecretError::Backend("write failed".into()));
        }
        lock(&self.stored).remove(account);
        Ok(())
    }
}
