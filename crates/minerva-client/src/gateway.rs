//! Authenticating gateway between a desktop webview and one registered server.
//!
//! Each connection is served to the webview from its own origin. The gateway
//! forwards that origin's requests to its server only, attaches the bearer token
//! from the credential store, captures tokens from sign-in responses so the page
//! never needs to hold them, and drops server cookies.
use crate::transport::{endpoint, network};
use crate::{
    normalize_server_url, Connection, CredentialStore, Error, Registry, Result, Transport,
};
use http::{HeaderValue, Method, Request, Response, StatusCode};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use url::Url;

pub(crate) const FORWARDED_REQUEST_HEADERS: [&str; 5] = [
    "accept",
    "accept-language",
    "content-type",
    "x-minerva-client",
    "range",
];
const DROPPED_RESPONSE_HEADERS: [&str; 5] = [
    "set-cookie",
    "connection",
    "transfer-encoding",
    "content-length",
    "keep-alive",
];

pub struct Gateway {
    transport: Transport,
    registry: Mutex<Registry>,
    credentials: Box<dyn CredentialStore>,
    tokens: Mutex<HashMap<String, Option<String>>>,
    verified: Mutex<HashSet<String>>,
}
impl Gateway {
    pub fn new(
        transport: Transport,
        registry: Registry,
        credentials: impl CredentialStore + 'static,
    ) -> Self {
        Self {
            transport,
            registry: Mutex::new(registry),
            credentials: Box::new(credentials),
            tokens: Mutex::new(HashMap::new()),
            verified: Mutex::new(HashSet::new()),
        }
    }
    pub fn connections(&self) -> Vec<Connection> {
        self.registry.lock().unwrap().connections().to_vec()
    }
    pub fn connection(&self, id: &str) -> Option<Connection> {
        self.registry.lock().unwrap().get(id).cloned()
    }
    fn require(&self, id: &str) -> Result<Connection> {
        self.connection(id)
            .ok_or_else(|| Error::Invalid("Unknown server connection".into()))
    }
    pub fn signed_in(&self, id: &str) -> bool {
        matches!(self.token(id), Ok(Some(_)))
    }
    fn token(&self, id: &str) -> Result<Option<String>> {
        if let Some(cached) = self.tokens.lock().unwrap().get(id) {
            return Ok(cached.clone());
        }
        let token = self.credentials.get(id)?;
        self.tokens.lock().unwrap().insert(id.into(), token.clone());
        Ok(token)
    }
    fn store_token(&self, id: &str, token: &str) -> Result<()> {
        self.credentials.set(id, token)?;
        self.tokens
            .lock()
            .unwrap()
            .insert(id.into(), Some(token.into()));
        Ok(())
    }
    fn forget_token(&self, id: &str) -> Result<()> {
        self.tokens.lock().unwrap().insert(id.into(), None);
        self.credentials.delete(id)
    }

    /// Registers a server after confirming it speaks API v1. Re-adding the same
    /// server at the same address returns the existing connection.
    pub async fn add(
        &self,
        address: &str,
        label: &str,
        allow_insecure_http: bool,
    ) -> Result<Connection> {
        let base = normalize_server_url(address, allow_insecure_http)?;
        let health = self.transport.health(&base).await?;
        if let Some(existing) = self
            .connections()
            .into_iter()
            .find(|c| c.url == base.as_str() && c.server_id == health.server_id)
        {
            return Ok(existing);
        }
        let label = label.trim();
        let connection = Connection {
            id: uuid::Uuid::new_v4().simple().to_string(),
            label: if label.is_empty() {
                base.host_str().unwrap_or("Minerva").to_string()
            } else {
                label.chars().take(80).collect()
            },
            url: base.to_string(),
            server_id: health.server_id,
            account: None,
            allow_insecure_http,
        };
        self.registry.lock().unwrap().upsert(connection.clone())?;
        self.verified.lock().unwrap().insert(connection.id.clone());
        Ok(connection)
    }
    pub fn rename(&self, id: &str, label: &str) -> Result<()> {
        let mut connection = self.require(id)?;
        let label = label.trim();
        if label.is_empty() {
            return Err(Error::Invalid("Enter a name".into()));
        }
        connection.label = label.chars().take(80).collect();
        self.registry.lock().unwrap().upsert(connection)
    }
    /// Confirms the server still reports the identity it was registered with.
    /// Tokens are never attached to requests for an unverified connection.
    pub async fn verify(&self, id: &str) -> Result<()> {
        let connection = self.require(id)?;
        let health = self.transport.health(&base(&connection)?).await?;
        if health.server_id != connection.server_id {
            self.verified.lock().unwrap().remove(id);
            return Err(Error::IdentityChanged {
                expected: connection.server_id,
                found: health.server_id,
            });
        }
        self.verified.lock().unwrap().insert(id.into());
        Ok(())
    }
    /// Accepts a changed server identity. The stored session belonged to the old
    /// server, so it is discarded and the user must sign in again.
    pub async fn trust_current_identity(&self, id: &str) -> Result<()> {
        let mut connection = self.require(id)?;
        let health = self.transport.health(&base(&connection)?).await?;
        self.forget_token(id)?;
        connection.server_id = health.server_id;
        connection.account = None;
        self.registry.lock().unwrap().upsert(connection)?;
        self.verified.lock().unwrap().insert(id.into());
        Ok(())
    }
    /// Revokes the session on the server when reachable and always removes it locally.
    pub async fn sign_out(&self, id: &str) -> Result<()> {
        let connection = self.require(id)?;
        let verified = self.verified.lock().unwrap().contains(id);
        if let (Some(token), true) = (self.token(id)?, verified) {
            let _ = self
                .transport
                .session(base(&connection)?, token)
                .logout()
                .await;
        }
        self.forget_token(id)
    }
    pub async fn remove(&self, id: &str) -> Result<()> {
        self.sign_out(id).await?;
        self.verified.lock().unwrap().remove(id);
        self.registry.lock().unwrap().remove(id)?;
        Ok(())
    }

    /// An authenticated API session for background work such as syncing. Fails
    /// with `Error::Api(401, _)` when the user is not signed in.
    pub async fn session(&self, id: &str) -> Result<crate::Session> {
        let connection = self.require(id)?;
        if !self.verified.lock().unwrap().contains(id) {
            self.verify(id).await?;
        }
        let token = self
            .token(id)?
            .ok_or_else(|| Error::Api(401, "Sign in to sync".into()))?;
        Ok(self.transport.session(base(&connection)?, token))
    }
    /// Forgets a token the server rejected.
    pub fn signed_out(&self, id: &str) -> Result<()> {
        self.forget_token(id)
    }
    /// Handles one webview request for connection `id`, served at `origin`.
    pub async fn handle(
        &self,
        id: &str,
        origin: &str,
        request: Request<Vec<u8>>,
    ) -> Response<Vec<u8>> {
        match self.forward(id, origin, request).await {
            Ok(response) => response,
            Err(Error::IdentityChanged { .. }) => error(
                StatusCode::MISDIRECTED_REQUEST,
                "This address now answers as a different Minerva server. Return to Servers to review it.",
            ),
            Err(Error::Network(message)) => error(StatusCode::BAD_GATEWAY, &format!("Unable to reach the server: {message}")),
            Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
        }
    }
    async fn forward(
        &self,
        id: &str,
        origin: &str,
        request: Request<Vec<u8>>,
    ) -> Result<Response<Vec<u8>>> {
        let Some(connection) = self.connection(id) else {
            return Ok(error(StatusCode::NOT_FOUND, "Unknown server connection"));
        };
        let (parts, body) = request.into_parts();
        let path = parts
            .uri
            .path_and_query()
            .map(|p| p.as_str())
            .unwrap_or("/")
            .to_string();
        // The static document may be opened from anywhere (e.g. the launcher);
        // everything it then requests must come from this connection's origin.
        let document = parts.method == Method::GET && parts.uri.path() == "/";
        if !(document || same_origin(&parts.headers, origin)) {
            return Ok(error(
                StatusCode::FORBIDDEN,
                "Cross-origin request rejected",
            ));
        }
        let method = match parts.method {
            Method::GET | Method::POST | Method::PUT | Method::PATCH | Method::DELETE => {
                parts.method.clone()
            }
            _ => return Ok(error(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed")),
        };
        if !self.verified.lock().unwrap().contains(id) {
            self.verify(id).await?;
        }
        // Sign-in completes at /login, or at /login/2fa after a second factor.
        let login = method == Method::POST
            && matches!(parts.uri.path(), "/api/v1/login" | "/api/v1/login/2fa");
        let logout = method == Method::POST && parts.uri.path() == "/api/v1/logout";
        let token = if login { None } else { self.token(id)? };

        let mut outgoing = self
            .transport
            .http
            .request(method, endpoint(&base(&connection)?, &path)?);
        for name in FORWARDED_REQUEST_HEADERS {
            if let Some(value) = parts.headers.get(name) {
                outgoing = outgoing.header(name, value);
            }
        }
        if let Some(token) = &token {
            outgoing = outgoing.bearer_auth(token);
        }
        if !body.is_empty() {
            outgoing = outgoing.body(body);
        }
        let upstream = outgoing.send().await.map_err(network)?;
        let status = upstream.status();
        let headers = upstream.headers().clone();
        let mut body = upstream.bytes().await.map_err(network)?.to_vec();

        if login && status.is_success() {
            body = self.capture_login(&connection, &body)?;
        }
        // Every app page (the main page, note tabs, share links) learns the
        // server's public address.
        let html = headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/html"));
        if html && status.is_success() {
            body = announce_server_origin(body, &base(&connection)?);
        }
        if logout || (status == StatusCode::UNAUTHORIZED && token.is_some()) {
            self.forget_token(id)?;
        }

        let mut response = Response::builder().status(status.as_u16());
        for (name, value) in headers.iter() {
            if !DROPPED_RESPONSE_HEADERS.contains(&name.as_str()) {
                response = response.header(name.as_str(), value.as_bytes());
            }
        }
        let mut response = response
            .body(body)
            .map_err(|_| Error::Network("Invalid upstream response".into()))?;
        response
            .headers_mut()
            .entry("cache-control")
            .or_insert(HeaderValue::from_static("no-store"));
        Ok(response)
    }
    /// Stores the issued token and removes it from what the page receives.
    fn capture_login(&self, connection: &Connection, body: &[u8]) -> Result<Vec<u8>> {
        let mut v: Value = serde_json::from_slice(body)
            .map_err(|_| Error::Network("Unexpected sign-in response".into()))?;
        // First step of a two-factor sign-in: no session yet.
        if v["two_factor_required"] == true {
            return Ok(body.to_vec());
        }
        let token = v
            .as_object_mut()
            .and_then(|o| o.remove("token"))
            .and_then(|t| t.as_str().map(String::from))
            .ok_or_else(|| Error::Network("Sign-in response did not include a token".into()))?;
        self.store_token(&connection.id, &token)?;
        if let Some(email) = v["user"]["email"].as_str() {
            let mut updated = connection.clone();
            updated.account = Some(email.into());
            self.registry.lock().unwrap().upsert(updated)?;
        }
        Ok(serde_json::to_vec(&v).unwrap_or_default())
    }
}

fn base(connection: &Connection) -> Result<Url> {
    normalize_server_url(&connection.url, connection.allow_insecure_http)
}
/// The page runs on a local origin, so it learns the server's public origin (for
/// example to show MCP configuration) from a meta element instead of `location`.
fn announce_server_origin(body: Vec<u8>, base: &Url) -> Vec<u8> {
    let html = match String::from_utf8(body) {
        Ok(html) => html,
        Err(e) => return e.into_bytes(),
    };
    let meta = format!(
        "<head>\n    <meta name=\"minerva-server-origin\" content=\"{}\" />",
        base.origin()
            .ascii_serialization()
            .replace(['"', '<', '>', '&'], "")
    );
    html.replacen("<head>", &meta, 1).into_bytes()
}
/// Requests from another connection's origin (or anywhere else) are refused, so one
/// server's pages cannot use another server's credentials.
pub(crate) fn same_origin(headers: &http::HeaderMap, origin: &str) -> bool {
    let text = |name| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(value) = text("origin") {
        if value != origin {
            return false;
        }
    }
    if let Some(referer) = text("referer") {
        match referer.strip_prefix(origin) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => {}
            _ => return false,
        }
    }
    true
}
fn error(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(json!({ "error": message }).to_string().into_bytes())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::same_origin;
    #[test]
    fn rejects_other_origins() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut h = http::HeaderMap::new();
            for (k, v) in pairs {
                h.insert(*k, v.parse().unwrap());
            }
            h
        };
        let me = "minerva://sabc";
        assert!(same_origin(&headers(&[]), me));
        assert!(same_origin(
            &headers(&[("origin", "minerva://sabc"), ("referer", "minerva://sabc/")]),
            me
        ));
        assert!(!same_origin(
            &headers(&[("origin", "minerva://sother")]),
            me
        ));
        assert!(!same_origin(
            &headers(&[("referer", "minerva://sabcd/")]),
            me
        ));
        assert!(!same_origin(&headers(&[("referer", "minerva://app/")]), me));
    }
}
