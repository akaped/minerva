//! Native client foundation: connection registry, OS credential storage, the
//! versioned JSON API transport, and the embedded local server with its offline
//! sync worker, shared by the Windows and Linux desktop clients.
//!
//! Every server connection is independently authenticated. Cached identity is
//! qualified by the connection and the server's persistent UUID; nothing is shared
//! between servers.
mod credentials;
mod gateway;
mod local;
mod registry;
mod sync;
mod transport;

pub use credentials::{CredentialStore, KeyringStore, MemoryStore};
pub use gateway::Gateway;
pub use local::LocalServer;
pub use registry::{Connection, Registry};
pub use sync::SyncWorker;
pub use transport::{normalize_server_url, Health, Session, Transport};

#[derive(Debug)]
pub enum Error {
    /// The server answered with an API error (`{"error": "..."}`) and status.
    Api(u16, String),
    /// The connection could not be established or the response was unreadable.
    Network(String),
    /// The server at this address no longer reports the identity it was added with.
    IdentityChanged {
        expected: String,
        found: String,
    },
    Invalid(String),
    Storage(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api(status, message) => write!(f, "{message} (HTTP {status})"),
            Error::Network(message) => write!(f, "Unable to reach the server: {message}"),
            Error::IdentityChanged { .. } => write!(
                f,
                "This address now answers as a different Minerva server. Verify the server before trusting it."
            ),
            Error::Invalid(message) | Error::Storage(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
