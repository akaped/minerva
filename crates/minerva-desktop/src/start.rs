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

/// Why the server open last was not opened again.
#[derive(Debug, PartialEq)]
enum Missed {
    /// Its session ended: sign in from the server list.
    SignedOut,
    /// No answer within a few seconds.
    Silent,
    /// It answered with an error, or could not be reached at all.
    Unreachable,
    /// Another server answers at its address.
    Moved,
}
impl Missed {
    fn name(&self) -> &'static str {
        match self {
            Missed::SignedOut => "signed_out",
            Missed::Silent => "timeout",
            Missed::Unreachable => "offline",
            Missed::Moved => "identity",
        }
    }
}

/// The page to open first: the server open last when it is still registered,
/// signed in, and answers with its registered identity within a few seconds.
/// Otherwise this computer's, which also works offline, and when that server is
/// still registered the page says why it is not shown (see `web/start-notice.js`).
pub fn page(data: &Path, gateway: &Gateway, runtime: &tokio::runtime::Runtime) -> String {
    let last = std::fs::read_to_string(file(data)).unwrap_or_default();
    let last = last.trim();
    let Some((id, connection)) =
        origin::connection_id(last).and_then(|id| gateway.connection(id).map(|c| (id, c)))
    else {
        return origin::page_url(origin::LOCAL, "/");
    };
    let missed = if !gateway.signed_in(id) {
        Some(Missed::SignedOut)
    } else {
        // The timeout's timer must be created inside the runtime.
        match runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), gateway.verify(id)).await
        }) {
            Ok(Ok(())) => None,
            Ok(Err(minerva_client::Error::IdentityChanged { .. })) => Some(Missed::Moved),
            Ok(Err(_)) => Some(Missed::Unreachable),
            Err(_) => Some(Missed::Silent),
        }
    };
    match missed {
        None => origin::page_url(last, "/"),
        Some(missed) => local_page(id, &connection.label, &missed),
    }
}
fn local_page(id: &str, label: &str, missed: &Missed) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("missed", missed.name())
        .append_pair("server", id)
        .append_pair("label", label)
        .finish();
    origin::page_url(origin::LOCAL, &format!("/?{query}"))
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
    #[test]
    fn says_why_the_last_server_is_not_shown() {
        let page = local_page("0123abcd", "Work & home", &Missed::Silent);
        assert_eq!(origin::page_host(&page).as_deref(), Some(origin::LOCAL));
        assert!(page.ends_with("/?missed=timeout&server=0123abcd&label=Work+%26+home"));
        assert_eq!(Missed::SignedOut.name(), "signed_out");
    }
}
