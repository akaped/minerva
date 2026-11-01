//! The server whose pages were open last, so the app starts there again. That
//! server's own page then reopens the workspace it showed last (see `web/app.js`).
use crate::origin;
use minerva_client::Gateway;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn file(data: &Path) -> PathBuf {
    data.join("last-server")
}

/// Records the server of a page that finished loading: this computer's or a
/// registered server's, never the launcher.
pub fn remember(data: &Path, url: &str) {
    let Some(host) = origin::page_host(url) else {
        return;
    };
    if host == origin::LOCAL || origin::connection_id(&host).is_some() {
        let _ = std::fs::write(file(data), host);
    }
}

/// The host to open first: the server open last when it is still registered,
/// signed in, and answers with its registered identity within a few seconds;
/// otherwise this computer's, which also works offline.
pub fn host(data: &Path, gateway: &Gateway, runtime: &tokio::runtime::Runtime) -> String {
    let last = std::fs::read_to_string(file(data)).unwrap_or_default();
    let last = last.trim();
    let reachable = origin::connection_id(last).is_some_and(|id| {
        gateway.connection(id).is_some()
            && gateway.signed_in(id)
            // The timeout's timer must be created inside the runtime.
            && runtime
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(3), gateway.verify(id)).await
                })
                .is_ok_and(|verified| verified.is_ok())
    });
    if reachable {
        last.to_string()
    } else {
        origin::LOCAL.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remembers_server_pages_only() {
        let data = std::env::temp_dir().join(format!("minerva-start-{}", std::process::id()));
        std::fs::create_dir_all(&data).unwrap();
        let read = || std::fs::read_to_string(file(&data)).unwrap_or_default();
        let server = origin::connection_host("0123abcd");
        remember(&data, &origin::page_url(&server, "/?workspace=x"));
        assert_eq!(read(), server);
        remember(&data, &origin::page_url(origin::LAUNCHER, "/"));
        remember(&data, "https://example.com/");
        assert_eq!(read(), server);
        remember(&data, &origin::page_url(origin::LOCAL, "/"));
        assert_eq!(read(), origin::LOCAL);
        std::fs::remove_dir_all(&data).unwrap();
    }
}
