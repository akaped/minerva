//! Personal API tokens: for scripts and the `minerva-cli` command, they sign in
//! as their owner through `Authorization: Bearer mnv_api_v1_…`, with the same
//! permissions, or only to read (`read_only`). They are named, expire (1–365
//! days) and can be revoked, and they cannot manage the account itself: tokens,
//! MCP credentials, password, second factor, signing out. Server administrator
//! accounts cannot have them.
//!
//! When a token or MCP credential was last used is kept in `token_use`, apart
//! from the data, so reading with one never rewrites it (see `guard`).
use super::*;
use axum::{extract::Request, middleware::Next};

pub(super) const PREFIX: &str = "mnv_api_v1_";
const LIMIT: usize = 20;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ApiToken {
    id: String,
    user_id: String,
    name: String,
    token_hash: String,
    created_at: u64,
    expires_at: u64,
    /// Reads only: anything but GET (and search) is refused.
    #[serde(default)]
    read_only: bool,
}
fn view(db: &Connection, t: &ApiToken) -> Value {
    json!({"id":t.id,"name":t.name,"created_at":t.created_at,"expires_at":t.expires_at,"access":if t.read_only {"read_only"} else {"read_write"},"last_used_at":last_used(db, &t.token_hash)})
}

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS token_use(token_hash TEXT PRIMARY KEY, at INTEGER NOT NULL);",
    )
}
/// When the token with this hash was last used, if ever.
pub(super) fn last_used(db: &Connection, hash: &str) -> Option<u64> {
    db.query_row(
        "SELECT at FROM token_use WHERE token_hash=?1",
        [hash],
        |r| r.get(0),
    )
    .ok()
}
/// Notes a use, at most once a minute per token.
fn touch(db: &Connection, hash: &str) {
    let _ = db.execute(
        "INSERT INTO token_use(token_hash, at) VALUES(?1, ?2)
        ON CONFLICT(token_hash) DO UPDATE SET at=excluded.at WHERE excluded.at >= token_use.at + 60",
        params![hash, now()],
    );
}
/// What a read-only token may still do besides reading: search.
fn reads(method: &axum::http::Method, path: &str) -> bool {
    matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS")
        || (method == axum::http::Method::POST && path == "/api/v1/search")
}
/// Every request: notes when a token or MCP credential is used, and keeps
/// read-only tokens to reading.
pub(super) async fn guard(
    axum::extract::State(store): axum::extract::State<Shared>,
    req: Request,
    next: Next,
) -> Response {
    let token = session_token(req.headers())
        .filter(|t| t.starts_with(PREFIX) || t.starts_with("mnv_mcp_v1_"))
        .map(hash_token);
    if let Some(hash) = token {
        let refused = {
            let Ok(s) = store.lock() else {
                return internal().into_response();
            };
            let current = s
                .data
                .api_tokens
                .iter()
                .find(|t| t.token_hash == hash && t.expires_at > now());
            if current.is_some() || s.data.mcp_credentials.iter().any(|c| c.hash() == hash) {
                touch(&s.db, &hash);
            }
            current.is_some_and(|t| t.read_only) && !reads(req.method(), req.uri().path())
        };
        if refused {
            return ApiError(
                StatusCode::FORBIDDEN,
                "This token is read only. Create one with write access to change anything.".into(),
            )
            .into_response();
        }
    }
    next.run(req).await
}

/// The user a personal token signs in as, if it is current.
pub(super) fn user(d: &Data, token: &str) -> Option<User> {
    let hash = hash_token(token);
    let t = d
        .api_tokens
        .iter()
        .find(|t| t.token_hash == hash && t.expires_at > now())?;
    d.users
        .iter()
        .find(|a| a.user.id == t.user_id && !a.user.disabled && (!a.user.admin || d.local))
        .map(|a| a.user.clone())
}
/// Whether the request signs in with a personal token.
pub(super) fn used(headers: &HeaderMap) -> bool {
    session_token(headers).is_some_and(|t| t.starts_with(PREFIX))
}
/// Paths a personal token may not use: the account's own security.
pub(super) fn reserved(parts: &[&str]) -> bool {
    matches!(
        parts.first(),
        Some(&("account" | "api-tokens" | "mcp-credentials" | "calendar-feed" | "logout"))
    )
}

/// `api-tokens`: list, create, revoke your tokens.
pub(super) fn route(
    db: &Connection,
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    match (method, parts) {
        ("GET", [_]) => Ok(json!(d
            .api_tokens
            .iter()
            .filter(|t| t.user_id == u.id)
            .map(|t| view(db, t))
            .collect::<Vec<_>>())),
        ("POST", [_]) => {
            if u.admin && !d.local {
                return Err(bad(
                    "Server administrator accounts cannot have API tokens; use a member account",
                ));
            }
            let name = field(v, "name")?.trim().to_string();
            if name.is_empty() || name.chars().count() > 80 {
                return Err(bad("A token has a name of up to 80 characters"));
            }
            let days = v
                .get("expires_days")
                .map(|v| v.as_u64().ok_or_else(|| bad("Expiry must be 1–365 days")))
                .transpose()?
                .unwrap_or(90);
            if !(1..=365).contains(&days) {
                return Err(bad("Expiry must be 1–365 days"));
            }
            let read_only = match v["access"].as_str().unwrap_or("read_write") {
                "read_only" => true,
                "read_write" => false,
                _ => return Err(bad("Access is read_only or read_write")),
            };
            d.api_tokens.retain(|t| t.expires_at > now());
            if d.api_tokens.iter().filter(|t| t.user_id == u.id).count() >= LIMIT {
                return Err(bad("Revoke an existing token before creating another"));
            }
            let token = format!("{PREFIX}{}{}", id(), id()).replace('-', "");
            let t = ApiToken {
                id: id(),
                user_id: u.id.clone(),
                name,
                token_hash: hash_token(&token),
                created_at: now(),
                expires_at: now() + days * 86400,
                read_only,
            };
            let mut result = view(db, &t);
            // Shown once: only its hash is kept.
            result["token"] = json!(token);
            d.api_tokens.push(t);
            Ok(result)
        }
        ("DELETE", [_, tid]) => {
            let before = d.api_tokens.len();
            d.api_tokens
                .retain(|t| !(t.id == *tid && t.user_id == u.id));
            if d.api_tokens.len() == before {
                return Err(missing());
            }
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}
/// Revokes every token of a deleted account.
pub(super) fn forget_user(d: &mut Data, user_id: &str) {
    d.api_tokens.retain(|t| t.user_id != user_id);
}
