//! Webview origins. The launcher and every server connection are served from
//! separate origins so that one server's pages can never reach another's
//! credentials or the launcher's commands.
//!
//! Linux (WebKitGTK) uses the `minerva://<host>` scheme directly. WebView2 only
//! intercepts HTTP(S), so Windows pages live at `https://minerva.<host>`; wry
//! converts those back to `minerva://<host>` before calling the protocol handler.
use url::Url;

pub const SCHEME: &str = "minerva";
pub const LAUNCHER: &str = "app.localhost";
/// This computer's own server: personal notes and offline copies.
pub const LOCAL: &str = "local.localhost";

pub fn connection_host(id: &str) -> String {
    format!("s{id}.localhost")
}
pub fn connection_id(host: &str) -> Option<&str> {
    host.strip_suffix(".localhost")?
        .strip_prefix('s')
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
}
/// The origin the webview reports (in `Origin`/`Referer`) for pages on `host`.
pub fn origin(host: &str) -> String {
    if cfg!(windows) {
        format!("https://{SCHEME}.{host}")
    } else {
        format!("{SCHEME}://{host}")
    }
}
pub fn page_url(host: &str, path: &str) -> String {
    format!("{}{path}", origin(host))
}
/// Returns the local host a webview URL belongs to, or `None` for external URLs.
pub fn page_host(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let host = url.host_str()?;
    if url.scheme() == SCHEME {
        return Some(host.to_string());
    }
    if cfg!(windows) && url.scheme() == "https" {
        return host
            .strip_prefix("minerva.")
            .filter(|h| h.ends_with(".localhost"))
            .map(String::from);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_connections_to_isolated_hosts() {
        let host = connection_host("0123abcd");
        assert_eq!(connection_id(&host), Some("0123abcd"));
        assert_eq!(connection_id(LAUNCHER), None);
        assert_eq!(connection_id(LOCAL), None);
        assert_eq!(connection_id("s.localhost"), None);
        assert_eq!(connection_id("s../x.localhost"), None);
        assert_eq!(
            page_host(&page_url(&host, "/api/v1/bootstrap")).as_deref(),
            Some(host.as_str())
        );
        assert_eq!(
            page_host(&page_url(LAUNCHER, "/")).as_deref(),
            Some(LAUNCHER)
        );
        assert_eq!(page_host("https://notes.example.com/"), None);
        assert_eq!(page_host("https://minerva.example.com/"), None);
    }
}
