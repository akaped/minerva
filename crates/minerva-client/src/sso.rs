//! Single sign-on for the desktop app, the way RFC 8252 asks native apps to do
//! it: the system browser signs in, never the app's webview.
//!
//! The app makes a secret verifier and listens on a free port of 127.0.0.1. It
//! opens `/auth/sso/start` in the browser with that port and the verifier's
//! SHA-256 (the challenge). After the provider, the server sends the browser to
//! `http://127.0.0.1:{port}/sso` with a one-time code (or with `error`), and
//! the app exchanges the code and the verifier at `POST /api/v1/login/sso`. A
//! code that leaks to another program on this computer is useless without the
//! verifier.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// As long as the server keeps a sign-in at the provider.
pub(crate) const WAIT: Duration = Duration::from_secs(600);

/// A new verifier (64 characters) and its challenge.
pub(crate) fn verifier() -> (String, String) {
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// Waits for the browser to come back to `/sso`: `Ok(code)`, or `Err(message)`
/// when the server says why the sign-in stopped. Other requests (a favicon, a
/// stray connection) are answered and ignored.
pub(crate) async fn wait(listener: &TcpListener) -> std::io::Result<Result<String, String>> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let Ok(Ok(target)) =
            tokio::time::timeout(Duration::from_secs(5), request_target(&mut stream)).await
        else {
            continue;
        };
        let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
            reply(&mut stream, 400, "Bad request", "").await;
            continue;
        };
        if url.path() != "/sso" {
            reply(&mut stream, 404, "Not found", "").await;
            continue;
        }
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        if let Some(code) = q.get("code").filter(|c| !c.is_empty()) {
            reply(
                &mut stream,
                200,
                "Signed in",
                "You are signed in to Minerva. You can close this tab and return to the app.",
            )
            .await;
            return Ok(Ok(code.clone()));
        }
        if let Some(message) = q.get("error") {
            reply(&mut stream, 200, "Sign-in did not work", message).await;
            return Ok(Err(message.clone()));
        }
        reply(&mut stream, 400, "Bad request", "").await;
    }
}
/// The path and query of an HTTP request's first line.
async fn request_target(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") && buffer.len() < 8192 {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buffer);
    let line = text.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) if target.starts_with('/') => Ok(target.to_string()),
        _ => Err(std::io::Error::other("not a GET request")),
    }
}
async fn reply(stream: &mut TcpStream, status: u16, title: &str, message: &str) {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body style=\"font-family:system-ui,sans-serif;max-width:520px;margin:48px auto;padding:0 16px;line-height:1.5\"><h1>{title}</h1><p>{message}</p></body></html>",
        title = escape(title),
        message = escape(message)
    );
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(port: u16, target: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn the_browser_brings_back_a_code_or_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(async move { wait(&listener).await.unwrap() });
        assert!(get(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        let page = get(port, "/sso?code=abc").await;
        assert!(page.contains("close this tab"));
        assert_eq!(waiting.await.unwrap(), Ok("abc".to_string()));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(async move { wait(&listener).await.unwrap() });
        let page = get(port, "/sso?error=No%20account%20%3Cb%3E").await;
        assert!(page.contains("No account &lt;b&gt;"), "{page}");
        assert_eq!(waiting.await.unwrap(), Err("No account <b>".to_string()));
    }

    #[test]
    fn the_challenge_is_the_verifier_hashed() {
        let (verifier, challenge) = verifier();
        assert_eq!(verifier.len(), 64);
        assert_eq!(challenge.len(), 43);
        assert_eq!(
            challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        );
    }
}
