use std::sync::{Arc, OnceLock};

use keyring_core::{Entry, Error, api::CredentialStore};

use super::{SERVICE, SecretBackend, SecretError};

pub(super) struct KeyringBackend {
    store: OnceLock<Result<Arc<CredentialStore>, String>>,
}

impl KeyringBackend {
    pub(super) fn lazy() -> Self {
        Self { store: OnceLock::new() }
    }

    #[cfg(test)]
    fn with_store(store: Arc<CredentialStore>) -> Self {
        let cell = OnceLock::new();
        let _ = cell.set(Ok(store));
        Self { store: cell }
    }

    fn store(&self) -> Result<&Arc<CredentialStore>, SecretError> {
        let opened = self.store.get_or_init(|| {
            open_store().map_err(|err| {
                tracing::info!("no system keyring: {err}");
                err.to_string()
            })
        });
        opened.as_ref().map_err(|message| SecretError::Backend(message.clone()))
    }

    fn entry(&self, account: &str) -> Result<Entry, SecretError> {
        self.store()?.build(SERVICE, account, None).map_err(backend_error)
    }
}

impl Drop for KeyringBackend {
    fn drop(&mut self) {
        if let Some(Ok(store)) = self.store.take() {
            std::thread::spawn(move || drop(store));
        }
    }
}

#[cfg(target_os = "linux")]
fn open_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(zbus_secret_service_keyring_store::Store::new()?)
}

#[cfg(target_os = "macos")]
fn open_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Err(Error::NotSupportedByStore("no system keyring on this platform".into()))
}

fn backend_error(err: Error) -> SecretError {
    SecretError::Backend(err.to_string())
}

impl SecretBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(Error::NoEntry) => Ok(None),
            Err(err) => Err(backend_error(err)),
        }
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.entry(account)?.set_password(secret).map_err(backend_error)
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(err) => Err(backend_error(err)),
        }
    }
}

#[cfg(test)]
mod tests;
