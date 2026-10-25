//! Session tokens live in Windows Credential Manager or the Linux Secret Service,
//! never in configuration files. Passwords are never stored.
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::Mutex;

pub trait CredentialStore: Send + Sync {
    fn get(&self, connection_id: &str) -> Result<Option<String>>;
    fn set(&self, connection_id: &str, token: &str) -> Result<()>;
    fn delete(&self, connection_id: &str) -> Result<()>;
}

pub struct KeyringStore {
    service: String,
}
impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
    fn entry(&self, connection_id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, &format!("connection:{connection_id}")).map_err(storage)
    }
}
fn storage(e: keyring::Error) -> Error {
    Error::Storage(format!("System credential store unavailable: {e}"))
}
impl CredentialStore for KeyringStore {
    fn get(&self, connection_id: &str) -> Result<Option<String>> {
        match self.entry(connection_id)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(storage(e)),
        }
    }
    fn set(&self, connection_id: &str, token: &str) -> Result<()> {
        self.entry(connection_id)?
            .set_password(token)
            .map_err(storage)
    }
    fn delete(&self, connection_id: &str) -> Result<()> {
        match self.entry(connection_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(storage(e)),
        }
    }
}

/// Process-local store for tests and for systems without a credential service.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);
impl CredentialStore for MemoryStore {
    fn get(&self, connection_id: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(connection_id).cloned())
    }
    fn set(&self, connection_id: &str, token: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(connection_id.into(), token.into());
        Ok(())
    }
    fn delete(&self, connection_id: &str) -> Result<()> {
        self.0.lock().unwrap().remove(connection_id);
        Ok(())
    }
}
