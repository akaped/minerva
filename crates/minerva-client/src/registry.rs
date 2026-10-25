//! Registry of independently authenticated server connections, persisted as JSON
//! in the user's configuration directory. Contains no secrets.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Connection {
    /// Local identifier; also names the webview origin and the credential entry.
    pub id: String,
    pub label: String,
    /// Normalized base URL ending in `/`.
    pub url: String,
    /// Persistent server UUID reported by `/api/v1/health` when the connection was added.
    pub server_id: String,
    #[serde(default)]
    pub account: Option<String>,
    /// TLS policy: plain HTTP is accepted only for loopback hosts unless this is set.
    #[serde(default)]
    pub allow_insecure_http: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    version: u32,
    connections: Vec<Connection>,
}

pub struct Registry {
    path: Option<PathBuf>,
    connections: Vec<Connection>,
}
impl Registry {
    /// Loads the registry, starting empty when the file does not exist yet.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let connections = match std::fs::read(&path) {
            Ok(raw) => {
                serde_json::from_slice::<File>(&raw)
                    .map_err(|e| {
                        Error::Storage(format!("{} is not a valid registry: {e}", path.display()))
                    })?
                    .connections
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(Error::Storage(format!("{}: {e}", path.display()))),
        };
        Ok(Self {
            path: Some(path),
            connections,
        })
    }
    pub fn in_memory() -> Self {
        Self {
            path: None,
            connections: Vec::new(),
        }
    }
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }
    pub fn get(&self, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }
    pub fn upsert(&mut self, connection: Connection) -> Result<()> {
        match self.connections.iter_mut().find(|c| c.id == connection.id) {
            Some(existing) => *existing = connection,
            None => self.connections.push(connection),
        }
        self.save()
    }
    pub fn remove(&mut self, id: &str) -> Result<Option<Connection>> {
        let index = self.connections.iter().position(|c| c.id == id);
        let removed = index.map(|i| self.connections.remove(i));
        self.save()?;
        Ok(removed)
    }
    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = File {
            version: 1,
            connections: self.connections.clone(),
        };
        let raw = serde_json::to_vec_pretty(&file).map_err(|e| Error::Storage(e.to_string()))?;
        write_atomically(path, &raw).map_err(|e| Error::Storage(format!("{}: {e}", path.display())))
    }
}
fn write_atomically(path: &Path, raw: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, raw)?;
    std::fs::rename(&temporary, path)
}
