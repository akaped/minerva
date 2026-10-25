//! HTTP transport for API v1. Authentication uses `Authorization: Bearer`; cookies
//! are never stored or sent.
use crate::{Error, Result};
use minerva_core::{Note, SaveNote};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use url::Url;

#[derive(Clone, Deserialize, Debug)]
pub struct Health {
    pub status: String,
    pub api_version: u32,
    pub server_id: String,
}

/// Parses user input into a base URL ending in `/`. Plain HTTP is only accepted for
/// loopback addresses unless `allow_insecure_http` is set.
pub fn normalize_server_url(input: &str, allow_insecure_http: bool) -> Result<Url> {
    let input = input.trim();
    let with_scheme = if input.contains("://") {
        input.to_string()
    } else if is_loopback_text(input) {
        format!("http://{input}")
    } else {
        format!("https://{input}")
    };
    let mut url = Url::parse(&with_scheme)
        .map_err(|_| Error::Invalid("Enter a valid server address".into()))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Invalid(
            "Server addresses must not contain credentials".into(),
        ));
    }
    match url.scheme() {
        "https" => {}
        "http" if allow_insecure_http || is_loopback(&url) => {}
        "http" => {
            return Err(Error::Invalid(
                "Use HTTPS for remote servers, or explicitly allow insecure HTTP".into(),
            ))
        }
        _ => return Err(Error::Invalid("Server addresses must use HTTP(S)".into())),
    }
    if url.host_str().is_none() {
        return Err(Error::Invalid("Enter a valid server address".into()));
    }
    url.set_query(None);
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}
fn is_loopback_text(input: &str) -> bool {
    let host = input.split(['/', ':']).next().unwrap_or("");
    host == "localhost" || host.starts_with("127.") || input.starts_with("[::1]")
}
pub(crate) fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[derive(Clone)]
pub struct Transport {
    pub(crate) http: reqwest::Client,
}
impl Transport {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("minerva-desktop/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            // Redirects could forward the bearer token to another origin.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(Self { http })
    }
    pub async fn health(&self, base: &Url) -> Result<Health> {
        let response = self
            .http
            .get(endpoint(base, "api/v1/health")?)
            .send()
            .await
            .map_err(network)?;
        let health: Health = decode(response).await?;
        if health.api_version != 1 {
            return Err(Error::Invalid(format!(
                "Unsupported Minerva API version {}",
                health.api_version
            )));
        }
        Ok(health)
    }
    pub fn session(&self, base: Url, token: impl Into<String>) -> Session {
        Session {
            transport: self.clone(),
            base,
            token: token.into(),
        }
    }
    /// Signs in and returns an authenticated session; the password is not retained.
    /// Accounts with two-factor authentication need [`Transport::login_with_code`].
    pub async fn login(&self, base: &Url, email: &str, password: &str) -> Result<Session> {
        self.login_with_code(base, email, password, None).await
    }
    /// Signs in with an authenticator or recovery code when the account requires one.
    pub async fn login_with_code(
        &self,
        base: &Url,
        email: &str,
        password: &str,
        code: Option<&str>,
    ) -> Result<Session> {
        let response = self
            .http
            .post(endpoint(base, "api/v1/login")?)
            .header("x-minerva-client", "1")
            .json(&json!({ "email": email, "password": password }))
            .send()
            .await
            .map_err(network)?;
        let mut v: Value = decode(response).await?;
        if v["two_factor_required"] == true {
            let code = code.ok_or_else(|| {
                Error::Invalid("This account requires a code from an authenticator app".into())
            })?;
            let response = self
                .http
                .post(endpoint(base, "api/v1/login/2fa")?)
                .header("x-minerva-client", "1")
                .json(&json!({ "challenge": v["challenge"], "code": code }))
                .send()
                .await
                .map_err(network)?;
            v = decode(response).await?;
        }
        let token = v["token"]
            .as_str()
            .ok_or_else(|| Error::Invalid("Sign-in response did not include a token".into()))?;
        Ok(self.session(base.clone(), token))
    }
}

/// Typed access to API v1 for one authenticated connection.
pub struct Session {
    transport: Transport,
    base: Url,
    token: String,
}
impl Session {
    pub fn token(&self) -> &str {
        &self.token
    }
    async fn call<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let mut request = self
            .transport
            .http
            .request(
                method.clone(),
                endpoint(&self.base, &format!("api/v1/{path}"))?,
            )
            .bearer_auth(&self.token);
        if method != reqwest::Method::GET {
            request = request.header("x-minerva-client", "1");
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        decode(request.send().await.map_err(network)?).await
    }
    /// Any API v1 JSON request, for callers such as the sync worker.
    pub async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        self.call(method, path, body).await
    }
    /// Downloads an attachment's bytes.
    pub async fn attachment(&self, id: &str) -> Result<Vec<u8>> {
        let response = self
            .transport
            .http
            .get(endpoint(&self.base, &format!("api/v1/attachments/{id}"))?)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(network)?;
        if !response.status().is_success() {
            return Err(Error::Api(
                response.status().as_u16(),
                "Attachment unavailable".into(),
            ));
        }
        Ok(response.bytes().await.map_err(network)?.to_vec())
    }
    /// Stores an attachment under its existing ID (replication).
    pub async fn put_attachment(
        &self,
        note: &str,
        id: &str,
        filename: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let mut url = endpoint(&self.base, &format!("api/v1/notes/{note}/attachments/{id}"))?;
        url.query_pairs_mut().append_pair("filename", filename);
        decode::<Value>(
            self.transport
                .http
                .put(url)
                .bearer_auth(&self.token)
                .header("x-minerva-client", "1")
                .body(bytes)
                .send()
                .await
                .map_err(network)?,
        )
        .await
        .map(|_| ())
    }
    pub async fn bootstrap(&self) -> Result<Value> {
        self.call(reqwest::Method::GET, "bootstrap", None).await
    }
    pub async fn search(&self, query: &str) -> Result<Value> {
        self.call(
            reqwest::Method::POST,
            "search",
            Some(json!({ "query": query })),
        )
        .await
    }
    pub async fn note(&self, id: &str) -> Result<Note> {
        self.call(reqwest::Method::GET, &format!("notes/{id}"), None)
            .await
    }
    /// Saves a snapshot. HTTP 409 (`Error::Api(409, _)`) means another client saved first.
    pub async fn save_note(&self, id: &str, note: &SaveNote) -> Result<Note> {
        let body = serde_json::to_value(note).map_err(|e| Error::Invalid(e.to_string()))?;
        self.call(reqwest::Method::PUT, &format!("notes/{id}"), Some(body))
            .await
    }
    pub async fn logout(self) -> Result<()> {
        self.call::<Value>(reqwest::Method::POST, "logout", None)
            .await
            .map(|_| ())
    }
}

pub(crate) fn endpoint(base: &Url, path: &str) -> Result<Url> {
    base.join(path.trim_start_matches('/'))
        .map_err(|_| Error::Invalid("Invalid request path".into()))
}
pub(crate) fn network(e: reqwest::Error) -> Error {
    Error::Network(e.to_string())
}
async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(network)?;
    if !status.is_success() {
        let message = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v["error"].as_str().map(String::from))
            .unwrap_or_else(|| status.canonical_reason().unwrap_or("Request failed").into());
        return Err(Error::Api(status.as_u16(), message));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| Error::Network("The server returned an unexpected response".into()))
}

#[cfg(test)]
mod tests {
    use super::normalize_server_url;
    #[test]
    fn normalizes_addresses_and_enforces_tls_policy() {
        let url = |s, insecure| normalize_server_url(s, insecure).map(|u| u.to_string());
        assert_eq!(
            url("notes.example.com", false).unwrap(),
            "https://notes.example.com/"
        );
        assert_eq!(
            url("https://a.example/team?x=1#y", false).unwrap(),
            "https://a.example/team/"
        );
        assert_eq!(
            url("localhost:3000", false).unwrap(),
            "http://localhost:3000/"
        );
        assert_eq!(
            url("http://127.0.0.1:3000", false).unwrap(),
            "http://127.0.0.1:3000/"
        );
        assert!(url("http://notes.example.com", false).is_err());
        assert_eq!(
            url("http://10.0.0.5:3000", true).unwrap(),
            "http://10.0.0.5:3000/"
        );
        assert!(url("https://user:pw@a.example", false).is_err());
        assert!(url("file:///etc/passwd", true).is_err());
        assert!(url("", false).is_err());
    }
}
