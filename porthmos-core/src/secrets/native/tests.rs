use std::{any::Any, collections::HashMap, sync::mpsc, time::Duration};

use keyring_core::api::CredentialStoreApi;

use super::*;

struct WatchedStore {
    dropped: mpsc::Sender<bool>,
}

impl CredentialStoreApi for WatchedStore {
    fn vendor(&self) -> String {
        "test".to_string()
    }

    fn id(&self) -> String {
        "watched".to_string()
    }

    fn build(&self, _: &str, _: &str, _: Option<&HashMap<&str, &str>>) -> keyring_core::Result<Entry> {
        Err(Error::NoEntry)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Drop for WatchedStore {
    fn drop(&mut self) {
        let _ = self.dropped.send(tokio::runtime::Handle::try_current().is_ok());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_opened_store_is_never_dropped_on_a_runtime_thread() {
    let (sender, receiver) = mpsc::channel();
    let backend = KeyringBackend::with_store(Arc::new(WatchedStore { dropped: sender }));

    tokio::spawn(async move { drop(backend) }).await.unwrap();

    let dropped_inside_a_runtime = receiver.recv_timeout(Duration::from_secs(2)).expect("the store was dropped");
    assert!(!dropped_inside_a_runtime);
}

#[test]
fn the_opened_store_is_dropped_exactly_once_even_outside_a_runtime() {
    let (sender, receiver) = mpsc::channel();
    let backend = KeyringBackend::with_store(Arc::new(WatchedStore { dropped: sender }));

    drop(backend);

    assert!(!receiver.recv_timeout(Duration::from_secs(2)).expect("the store was dropped"));
    assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_backend_whose_store_was_never_opened_drops_quietly() {
    drop(KeyringBackend::lazy());
}
