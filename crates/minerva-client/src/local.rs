//! The desktop app's embedded server: an in-process `minerva-server` that holds
//! personal workspaces and replicas of remote ones, so the app works without a
//! network. Nothing listens on a port; the webview and the sync worker call the
//! router directly, with a session token that never leaves this process.
use crate::gateway::{same_origin, FORWARDED_REQUEST_HEADERS};
use crate::{Error, Result};
use axum::body::Body;
use http::{HeaderValue, Method, Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::path::Path;
use tower::ServiceExt;

pub struct LocalServer {
    router: axum::Router,
    token: String,
}
impl LocalServer {
    /// Opens (or creates) the local database.
    pub fn open(path: &Path) -> Result<Self> {
        let path = path
            .to_str()
            .ok_or_else(|| Error::Storage("The data directory path is not valid UTF-8".into()))?;
        let (store, token) = minerva_server::Store::open_local(path)
            .map_err(|e| Error::Storage(format!("{path}: {e}")))?;
        Ok(Self {
            router: minerva_server::app(store),
            token,
        })
    }
    async fn send(&self, request: Request<Body>) -> (StatusCode, http::HeaderMap, Vec<u8>) {
        let response = match self.router.clone().oneshot(request).await {
            Ok(response) => response,
            Err(never) => match never {},
        };
        let (parts, body) = response.into_parts();
        let bytes = body
            .collect()
            .await
            .map(|b| b.to_bytes().to_vec())
            .unwrap_or_default();
        (parts.status, parts.headers, bytes)
    }
    /// Serves one webview request from the local origin.
    pub async fn handle(&self, origin: &str, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
        let (parts, body) = request.into_parts();
        let document = parts.method == Method::GET && parts.uri.path() == "/";
        if !(document || same_origin(&parts.headers, origin)) {
            return error(StatusCode::FORBIDDEN, "Cross-origin request rejected");
        }
        let path = parts
            .uri
            .path_and_query()
            .map_or("/", |p| p.as_str())
            .to_string();
        // Signing in or out does not apply to the local owner.
        if matches!(
            parts.uri.path(),
            "/api/v1/logout" | "/api/v1/login" | "/api/v1/login/2fa"
        ) {
            return error(
                StatusCode::FORBIDDEN,
                "This app's own notes need no sign-in",
            );
        }
        let mut outgoing = Request::builder().method(parts.method).uri(path);
        for name in FORWARDED_REQUEST_HEADERS {
            if let Some(value) = parts.headers.get(name) {
                outgoing = outgoing.header(name, value);
            }
        }
        let outgoing = outgoing
            .header("authorization", format!("Bearer {}", self.token))
            .body(Body::from(body));
        let Ok(outgoing) = outgoing else {
            return error(StatusCode::BAD_REQUEST, "Invalid request");
        };
        let (status, headers, body) = self.send(outgoing).await;
        let mut response = Response::builder().status(status);
        for (name, value) in headers.iter() {
            if !matches!(
                name.as_str(),
                "set-cookie" | "content-length" | "transfer-encoding"
            ) {
                response = response.header(name, value);
            }
        }
        let mut response = response
            .body(body)
            .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "Invalid response"));
        response
            .headers_mut()
            .entry("cache-control")
            .or_insert(HeaderValue::from_static("no-store"));
        response
    }
    /// One API v1 request (`path` without `/api/v1/`), with a JSON or raw body.
    async fn request(&self, method: Method, path: &str, body: Body) -> Result<Vec<u8>> {
        let request = Request::builder()
            .method(method)
            .uri(format!("/api/v1/{path}"))
            .header("content-type", "application/json")
            .header("x-minerva-client", "1")
            .header("authorization", format!("Bearer {}", self.token))
            .body(body)
            .map_err(|_| Error::Invalid("Invalid request path".into()))?;
        let (status, _, bytes) = self.send(request).await;
        if !status.is_success() {
            let message = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v["error"].as_str().map(String::from))
                .unwrap_or_else(|| "Local storage request failed".into());
            return Err(Error::Api(status.as_u16(), message));
        }
        Ok(bytes)
    }
    pub async fn call(&self, method: Method, path: &str, body: Value) -> Result<Value> {
        let body = if body.is_null() {
            Body::empty()
        } else {
            Body::from(body.to_string())
        };
        let bytes = self.request(method, path, body).await?;
        serde_json::from_slice(&bytes)
            .map_err(|_| Error::Storage("Unexpected local storage response".into()))
    }
    pub async fn attachment(&self, id: &str) -> Result<Vec<u8>> {
        self.request(Method::GET, &format!("attachments/{id}"), Body::empty())
            .await
    }
    pub async fn put_attachment(
        &self,
        note: &str,
        id: &str,
        filename: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let filename: String = url::form_urlencoded::byte_serialize(filename.as_bytes()).collect();
        self.request(
            Method::PUT,
            &format!("notes/{note}/attachments/{id}?filename={filename}"),
            Body::from(bytes),
        )
        .await
        .map(|_| ())
    }
}

fn error(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(json!({ "error": message }).to_string().into_bytes())
        .unwrap()
}
