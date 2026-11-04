//! OAuth 2.1 for MCP clients that cannot take a pasted credential, as the MCP
//! authorization specification describes: the MCP endpoint points to its
//! protected-resource metadata, clients register themselves (dynamic client
//! registration, public clients only), and a person approves a client for one
//! workspace on a consent page, signed in as usual. Codes need PKCE (S256).
//!
//! An access token is an ordinary MCP credential that lasts an hour, so it shows
//! in the person's connections and revoking it there ends the client's access;
//! refresh tokens (30 days) rotate on each use. Off unless
//! `MINERVA_MCP_OAUTH=1`.
use super::*;
use axum::extract::{Form, Query};
use std::collections::HashMap as Map;

const ACCESS_SECONDS: u64 = 3600;
const REFRESH_SECONDS: u64 = 30 * 86400;
const CODE_SECONDS: u64 = 600;
const MAX_CLIENTS: i64 = 500;

pub(super) fn enabled() -> bool {
    std::env::var("MINERVA_MCP_OAUTH").as_deref() == Ok("1")
}

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS oauth_clients(id TEXT PRIMARY KEY, payload TEXT NOT NULL, created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS oauth_grants(hash TEXT PRIMARY KEY, kind TEXT NOT NULL, payload TEXT NOT NULL, expires_at INTEGER NOT NULL);",
    )
}

/// The server's public address: `MINERVA_PUBLIC_URL`, or the request's host.
fn base(headers: &HeaderMap) -> String {
    if let Ok(url) = std::env::var("MINERVA_PUBLIC_URL") {
        return url.trim_end_matches('/').to_string();
    }
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .filter(|h| {
            h.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
        })
        .unwrap_or("localhost:3000");
    format!("http://{host}")
}
fn json_response(status: StatusCode, body: Value) -> Response {
    (
        status,
        [("cache-control", "no-store"), ("pragma", "no-cache")],
        axum::Json(body),
    )
        .into_response()
}
fn oauth_error(error: &str, description: &str) -> Response {
    json_response(
        StatusCode::BAD_REQUEST,
        json!({"error": error, "error_description": description}),
    )
}
fn off() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

/// `WWW-Authenticate` for the MCP endpoint's 401s, when OAuth is on.
pub(super) fn challenge(headers: &HeaderMap, wid: &str) -> Option<String> {
    enabled().then(|| {
        format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/api/workspaces/{wid}/mcp\"",
            base(headers)
        )
    })
}

/// RFC 9728: which authorization server protects a workspace's MCP endpoint.
pub(super) async fn resource_metadata(
    axum::extract::Path(wid): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Response {
    if !enabled() {
        return off();
    }
    let base = base(&headers);
    json_response(
        StatusCode::OK,
        json!({
            "resource": format!("{base}/api/workspaces/{wid}/mcp"),
            "authorization_servers": [base],
            "scopes_supported": ["mcp:read", "mcp:write"],
            "bearer_methods_supported": ["header"],
        }),
    )
}
/// RFC 8414: the authorization server's endpoints.
pub(super) async fn server_metadata(headers: HeaderMap) -> Response {
    if !enabled() {
        return off();
    }
    let base = base(&headers);
    json_response(
        StatusCode::OK,
        json!({
            "issuer": base,
            "authorization_endpoint": format!("{base}/oauth/authorize"),
            "token_endpoint": format!("{base}/oauth/token"),
            "registration_endpoint": format!("{base}/oauth/register"),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "scopes_supported": ["mcp:read", "mcp:write"],
        }),
    )
}

#[derive(Serialize, Deserialize, Clone)]
struct Client {
    id: String,
    name: String,
    redirect_uris: Vec<String>,
}
/// A redirect is HTTPS, or HTTP to this computer (as desktop clients listen).
fn redirect_allowed(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host().is_some(),
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    }
}
/// RFC 7591: a public client registers its name and redirect addresses.
pub(super) async fn register(
    axum::extract::State(store): axum::extract::State<Shared>,
    body: axum::body::Bytes,
) -> Response {
    if !enabled() {
        return off();
    }
    let Ok(v) = serde_json::from_slice::<Value>(&body) else {
        return oauth_error("invalid_client_metadata", "Expected JSON");
    };
    let name: String = v["client_name"]
        .as_str()
        .unwrap_or("MCP client")
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect();
    let uris: Vec<String> = v["redirect_uris"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|u| u.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if uris.is_empty()
        || uris.len() > 5
        || !uris.iter().all(|u| u.len() <= 500 && redirect_allowed(u))
    {
        return oauth_error(
            "invalid_redirect_uri",
            "Redirect addresses must be HTTPS, or HTTP to localhost (up to 5)",
        );
    }
    if v["token_endpoint_auth_method"]
        .as_str()
        .is_some_and(|m| m != "none")
    {
        return oauth_error(
            "invalid_client_metadata",
            "Only public clients (token_endpoint_auth_method none) are supported",
        );
    }
    let client = Client {
        id: id(),
        name: if name.trim().is_empty() {
            "MCP client".into()
        } else {
            name
        },
        redirect_uris: uris,
    };
    let Ok(s) = store.lock() else {
        return internal().into_response();
    };
    let count: i64 =
        s.db.query_row("SELECT count(*) FROM oauth_clients", [], |r| r.get(0))
            .unwrap_or(0);
    if count >= MAX_CLIENTS {
        // The oldest client that no grant uses makes room.
        let _ = s.db.execute(
            "DELETE FROM oauth_clients WHERE id = (SELECT id FROM oauth_clients ORDER BY created_at LIMIT 1)",
            [],
        );
    }
    if s.db
        .execute(
            "INSERT INTO oauth_clients(id, payload, created_at) VALUES(?1, ?2, ?3)",
            params![client.id, json!(client).to_string(), now()],
        )
        .is_err()
    {
        return internal().into_response();
    }
    json_response(
        StatusCode::CREATED,
        json!({
            "client_id": client.id,
            "client_name": client.name,
            "redirect_uris": client.redirect_uris,
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "client_id_issued_at": now(),
        }),
    )
}
fn client(db: &Connection, id: &str) -> Option<Client> {
    db.query_row("SELECT payload FROM oauth_clients WHERE id=?1", [id], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .and_then(|p| serde_json::from_str(&p).ok())
}

/// A short-lived grant (`code`, `consent`) or a refresh token, by its hash.
fn keep_grant(
    db: &Connection,
    secret: &str,
    kind: &str,
    payload: &Value,
    seconds: u64,
) -> rusqlite::Result<()> {
    db.execute("DELETE FROM oauth_grants WHERE expires_at < ?1", [now()])?;
    db.execute(
        "INSERT INTO oauth_grants(hash, kind, payload, expires_at) VALUES(?1, ?2, ?3, ?4)",
        params![
            hash_token(secret),
            kind,
            payload.to_string(),
            now() + seconds
        ],
    )?;
    Ok(())
}
/// Takes a grant: it can be used once.
fn take_grant(db: &Connection, secret: &str, kind: &str) -> Option<Value> {
    let hash = hash_token(secret);
    let found: Option<String> = db
        .query_row(
            "SELECT payload FROM oauth_grants WHERE hash=?1 AND kind=?2 AND expires_at >= ?3",
            params![hash, kind, now()],
            |r| r.get(0),
        )
        .ok();
    let _ = db.execute("DELETE FROM oauth_grants WHERE hash=?1", [&hash]);
    found.and_then(|p| serde_json::from_str(&p).ok())
}
fn secret() -> String {
    format!("{}{}", id(), id()).replace('-', "")
}

/// The signed-in person, from the session cookie only (never a token).
fn signed_in(d: &Data, headers: &HeaderMap) -> Option<User> {
    let mut only_cookie = HeaderMap::new();
    if let Some(c) = headers.get("cookie") {
        only_cookie.insert("cookie", c.clone());
    }
    authenticate(d, &only_cookie).ok()
}
/// The workspace a client asks for, from the `resource` it names.
fn workspace_of(resource: &str, base: &str) -> Option<String> {
    let rest = resource
        .strip_prefix(base)?
        .strip_prefix("/api/workspaces/")?;
    let wid = rest.strip_suffix("/mcp")?;
    (!wid.is_empty() && !wid.contains('/')).then(|| wid.to_string())
}
fn page(lang: &str, title: &str, body: String) -> Response {
    page_to(lang, title, body, "")
}
/// A page whose form may lead on to `also` (the client's origin, after consent).
fn page_to(lang: &str, title: &str, body: String, also: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title><link rel=\"stylesheet\" href=\"/public.css\"><style>main{{max-width:520px;margin:48px auto;padding:0 16px;font-family:system-ui,sans-serif;line-height:1.5}}.box{{border:1px solid #ddd;border-radius:12px;padding:20px}}label{{display:block;margin:10px 0}}button{{font:inherit;padding:8px 16px;border-radius:8px;border:1px solid #bbb;background:#fff;cursor:pointer}}button.primary{{background:#2f3a52;color:#fff;border-color:#2f3a52}}.actions{{display:flex;gap:10px;justify-content:flex-end;margin-top:18px}}small{{color:#666}}</style></head><body><main><h1>{title}</h1>{body}</main></body></html>",
        title = escape(title)
    );
    (
        StatusCode::OK,
        [
            ("content-type", "text/html; charset=utf-8".to_string()),
            ("x-frame-options", "DENY".into()),
            ("content-security-policy", format!("default-src 'none'; style-src 'self' 'unsafe-inline'; form-action 'self' {also}; frame-ancestors 'none'")),
            ("cache-control", "no-store".into()),
        ],
        html,
    )
        .into_response()
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
/// Where a decision goes back to the client.
fn back(redirect_uri: &str, query: &[(&str, &str)]) -> Response {
    let mut url = match url::Url::parse(redirect_uri) {
        Ok(u) => u,
        Err(_) => return oauth_error("invalid_request", "Invalid redirect"),
    };
    {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in query {
            if !v.is_empty() {
                pairs.append_pair(k, v);
            }
        }
    }
    (
        StatusCode::FOUND,
        [
            ("location", url.to_string()),
            ("cache-control", "no-store".into()),
        ],
    )
        .into_response()
}

/// The request's parameters, checked: the client, its redirect, PKCE.
struct Ask {
    client: Client,
    redirect_uri: String,
    challenge: String,
    state: String,
    workspace: Option<String>,
    write: bool,
}
fn check_ask(db: &Connection, base: &str, q: &Map<String, String>) -> Result<Ask, String> {
    let get = |k: &str| q.get(k).cloned().unwrap_or_default();
    let client = client(db, &get("client_id")).ok_or(i18n::msg(
        "This application is not registered with Minerva.",
    ))?;
    let redirect_uri = get("redirect_uri");
    if !client.redirect_uris.contains(&redirect_uri) {
        return Err(
            i18n::msg("The application's return address does not match its registration.").into(),
        );
    }
    if get("response_type") != "code" {
        return Err(i18n::msg("Only the authorization code flow is supported.").into());
    }
    let challenge = get("code_challenge");
    if get("code_challenge_method") != "S256" || challenge.len() < 43 || challenge.len() > 128 {
        return Err(i18n::msg("The application must use PKCE (S256).").into());
    }
    let resource = get("resource");
    let workspace = if resource.is_empty() {
        None
    } else {
        Some(
            workspace_of(&resource, base)
                .ok_or(i18n::msg("The application asks for an unknown resource."))?,
        )
    };
    Ok(Ask {
        client,
        redirect_uri,
        challenge,
        state: get("state"),
        workspace,
        write: get("scope").split(' ').any(|s| s == "mcp:write"),
    })
}
/// The consent page: which application, which workspace, what access.
pub(super) async fn authorize(
    axum::extract::State(store): axum::extract::State<Shared>,
    headers: HeaderMap,
    Query(q): Query<Map<String, String>>,
) -> Response {
    if !enabled() {
        return off();
    }
    let base = base(&headers);
    let Ok(s) = store.lock() else {
        return internal().into_response();
    };
    let lang = i18n::for_request(&headers);
    let t = |text: &str| i18n::tr(&lang, text);
    let ask = match check_ask(&s.db, &base, &q) {
        Ok(a) => a,
        Err(e) => {
            return page(
                &lang,
                &i18n::tr(&lang, "Cannot connect"),
                format!("<p>{}</p>", escape(&t(&e))),
            )
        }
    };
    let Some(u) = signed_in(&s.data, &headers) else {
        return page(
            &lang,
            &i18n::tr(&lang, "Sign in to continue"),
            format!(
                "<p>{}</p><p><a href=\"{}/\" target=\"_blank\" rel=\"noopener\">{}</a></p><form method=\"get\">{}<div class=\"actions\"><button class=\"primary\">{}</button></div></form>",
                escape(&i18n::tr(&lang, "Sign in to Minerva in another tab, then come back here.")),
                escape(&base),
                escape(&i18n::tr(&lang, "Open Minerva")),
                q.iter()
                    .map(|(k, v)| format!("<input type=\"hidden\" name=\"{}\" value=\"{}\">", escape(k), escape(v)))
                    .collect::<String>(),
                escape(&i18n::tr(&lang, "I have signed in"))
            ),
        );
    };
    let workspaces: Vec<&Workspace> = s
        .data
        .workspaces
        .iter()
        .filter(|w| mcp::workspace_visible(&s.data, &u, &w.id))
        .filter(|w| ask.workspace.as_deref().is_none_or(|x| x == w.id))
        .collect();
    if workspaces.is_empty() {
        return page(
            &lang,
            &i18n::tr(&lang, "Cannot connect"),
            format!(
                "<p>{}</p>",
                escape(&i18n::tr(&lang, "You cannot open that workspace."))
            ),
        );
    }
    // The form's own one-time key: only this page can approve.
    let consent = secret();
    if keep_grant(
        &s.db,
        &consent,
        "consent",
        &json!({"user": u.id, "client": ask.client.id}),
        CODE_SECONDS,
    )
    .is_err()
    {
        return internal().into_response();
    }
    let host = url::Url::parse(&ask.redirect_uri)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_default();
    let choose = if workspaces.len() == 1 {
        format!(
            "<p>{} <strong>{}</strong></p><input type=\"hidden\" name=\"workspace\" value=\"{}\">",
            escape(&i18n::tr(&lang, "Workspace:")),
            escape(&workspaces[0].name),
            escape(&workspaces[0].id)
        )
    } else {
        format!(
            "<label>{}<br><select name=\"workspace\">{}</select></label>",
            escape(&i18n::tr(&lang, "Workspace")),
            workspaces
                .iter()
                .map(|w| format!(
                    "<option value=\"{}\">{}</option>",
                    escape(&w.id),
                    escape(&w.name)
                ))
                .collect::<String>()
        )
    };
    let hidden: String = q
        .iter()
        .map(|(k, v)| {
            format!(
                "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
                escape(k),
                escape(v)
            )
        })
        .collect();
    // The approval goes on to the client's own address.
    let origin = url::Url::parse(&ask.redirect_uri)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default();
    page_to(
        &lang,
        &i18n::tr(&lang, "Connect an AI tool"),
        format!(
            "<div class=\"box\"><p>{}</p><p><small>{}</small></p><form method=\"post\" action=\"/oauth/authorize\">{hidden}<input type=\"hidden\" name=\"consent\" value=\"{consent}\">{choose}<label><input type=\"radio\" name=\"access\" value=\"read_only\" {}> {}</label><label><input type=\"radio\" name=\"access\" value=\"read_write\" {}> {}</label><p><small>{}</small></p><div class=\"actions\"><button name=\"decision\" value=\"deny\">{}</button><button class=\"primary\" name=\"decision\" value=\"allow\">{}</button></div></form></div>",
            escape(&i18n::fill(&i18n::tr(&lang, "“{name}” wants to use your Minerva notes."), &[("name", &ask.client.name)])),
            escape(&i18n::fill(&i18n::tr(&lang, "It will return to {host}. Signed in as {email}."), &[("host", &host), ("email", &u.email)])),
            if ask.write { "" } else { "checked" },
            escape(&i18n::tr(&lang, "Read only")),
            if ask.write { "checked" } else { "" },
            escape(&i18n::tr(&lang, "Read and write")),
            escape(&i18n::tr(&lang, "Your folder permissions always apply. You can revoke the connection under MCP connections.")),
            escape(&i18n::tr(&lang, "Deny")),
            escape(&i18n::tr(&lang, "Allow")),
        ),
        &origin,
    )
}
/// The consent page's answer.
pub(super) async fn decide(
    axum::extract::State(store): axum::extract::State<Shared>,
    headers: HeaderMap,
    Form(form): Form<Map<String, String>>,
) -> Response {
    if !enabled() {
        return off();
    }
    let base = base(&headers);
    // Only this server's page may post here.
    if let Some(origin) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        if origin != base && origin != "null" {
            return (StatusCode::FORBIDDEN, "Forbidden").into_response();
        }
    }
    let Ok(s) = store.lock() else {
        return internal().into_response();
    };
    let ask = match check_ask(&s.db, &base, &form) {
        Ok(a) => a,
        Err(e) => return oauth_error("invalid_request", &e),
    };
    let Some(u) = signed_in(&s.data, &headers) else {
        return (StatusCode::UNAUTHORIZED, "Please sign in").into_response();
    };
    let consent = take_grant(
        &s.db,
        form.get("consent").map_or("", String::as_str),
        "consent",
    );
    if consent.is_none_or(|c| c["user"] != u.id.as_str() || c["client"] != ask.client.id.as_str()) {
        return (
            StatusCode::FORBIDDEN,
            "This page has expired; start again from the application.",
        )
            .into_response();
    }
    if form.get("decision").map(String::as_str) != Some("allow") {
        return back(
            &ask.redirect_uri,
            &[("error", "access_denied"), ("state", &ask.state)],
        );
    }
    let wid = form.get("workspace").cloned().unwrap_or_default();
    if ask.workspace.as_deref().is_some_and(|w| w != wid)
        || !mcp::workspace_visible(&s.data, &u, &wid)
    {
        return back(
            &ask.redirect_uri,
            &[("error", "access_denied"), ("state", &ask.state)],
        );
    }
    let write = form.get("access").map(String::as_str) == Some("read_write");
    let code = secret();
    let grant = json!({
        "client": ask.client.id,
        "redirect_uri": ask.redirect_uri,
        "challenge": ask.challenge,
        "user": u.id,
        "workspace": wid,
        "write": write,
    });
    if keep_grant(&s.db, &code, "code", &grant, CODE_SECONDS).is_err() {
        return internal().into_response();
    }
    back(&ask.redirect_uri, &[("code", &code), ("state", &ask.state)])
}
fn pkce_ok(challenge: &str, verifier: &str) -> bool {
    use base64::Engine;
    let valid = (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b));
    valid
        && base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()))
            == challenge
}
/// Codes and refresh tokens for access tokens.
pub(super) async fn token(
    axum::extract::State(store): axum::extract::State<Shared>,
    Form(form): Form<Map<String, String>>,
) -> Response {
    if !enabled() {
        return off();
    }
    let get = |k: &str| form.get(k).cloned().unwrap_or_default();
    let Ok(mut s) = store.lock() else {
        return internal().into_response();
    };
    let client_id = get("client_id");
    let Some(client) = client(&s.db, &client_id) else {
        return oauth_error("invalid_client", "Unknown client");
    };
    let (credential, refresh_payload) = match get("grant_type").as_str() {
        "authorization_code" => {
            let Some(g) = take_grant(&s.db, &get("code"), "code") else {
                return oauth_error("invalid_grant", "The code is unknown, used or expired");
            };
            if g["client"] != client.id.as_str()
                || g["redirect_uri"] != get("redirect_uri").as_str()
                || !pkce_ok(g["challenge"].as_str().unwrap_or(""), &get("code_verifier"))
            {
                return oauth_error("invalid_grant", "The code does not match this request");
            }
            let user = g["user"].as_str().unwrap_or("").to_string();
            let wid = g["workspace"].as_str().unwrap_or("").to_string();
            let write = g["write"] == true;
            let mut d = s.data.clone();
            let Some(u) = d
                .users
                .iter()
                .find(|a| a.user.id == user && !a.user.disabled)
                .map(|a| a.user.clone())
            else {
                return oauth_error("invalid_grant", "The account is not available");
            };
            if !mcp::workspace_visible(&d, &u, &wid) {
                return oauth_error("invalid_grant", "The workspace is not available");
            }
            let issued = mcp::issue(
                &mut d,
                &user,
                &wid,
                &format!("{} (OAuth)", client.name),
                write,
                ACCESS_SECONDS,
            );
            let Ok(issued) = issued else {
                return oauth_error(
                    "invalid_grant",
                    "Revoke an existing connection before adding another",
                );
            };
            if persist(&mut s, d).is_err() {
                return internal().into_response();
            }
            let payload = json!({"client": client.id, "credential": issued.0, "write": write});
            (issued, payload)
        }
        "refresh_token" => {
            let Some(g) = take_grant(&s.db, &get("refresh_token"), "refresh") else {
                return oauth_error(
                    "invalid_grant",
                    "The refresh token is unknown, used or expired",
                );
            };
            if g["client"] != client.id.as_str() {
                return oauth_error(
                    "invalid_grant",
                    "The refresh token belongs to another client",
                );
            }
            let mut d = s.data.clone();
            // A revoked connection cannot be refreshed.
            let Some(issued) = mcp::renew(
                &mut d,
                g["credential"].as_str().unwrap_or(""),
                ACCESS_SECONDS,
            ) else {
                return oauth_error("invalid_grant", "The connection was revoked");
            };
            if persist(&mut s, d).is_err() {
                return internal().into_response();
            }
            (issued, g)
        }
        _ => {
            return oauth_error(
                "unsupported_grant_type",
                "Use authorization_code or refresh_token",
            )
        }
    };
    let refresh = secret();
    if keep_grant(
        &s.db,
        &refresh,
        "refresh",
        &refresh_payload,
        REFRESH_SECONDS,
    )
    .is_err()
    {
        return internal().into_response();
    }
    let write = refresh_payload["write"] == true;
    json_response(
        StatusCode::OK,
        json!({
            "access_token": credential.1,
            "token_type": "Bearer",
            "expires_in": ACCESS_SECONDS,
            "refresh_token": refresh,
            "scope": if write { "mcp:read mcp:write" } else { "mcp:read" },
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirects_and_resources() {
        for ok in [
            "https://app.example/cb",
            "http://localhost:4567/callback",
            "http://127.0.0.1/cb",
        ] {
            assert!(redirect_allowed(ok), "{ok}");
        }
        for bad in [
            "http://evil.example/cb",
            "javascript:alert(1)",
            "https://x.example/cb#frag",
            "https://u:p@x.example/",
        ] {
            assert!(!redirect_allowed(bad), "{bad}");
        }
        assert_eq!(
            workspace_of(
                "https://m.example/api/workspaces/abc/mcp",
                "https://m.example"
            )
            .as_deref(),
            Some("abc")
        );
        assert_eq!(
            workspace_of(
                "https://other.example/api/workspaces/abc/mcp",
                "https://m.example"
            ),
            None
        );
        assert_eq!(
            workspace_of(
                "https://m.example/api/workspaces/a/b/mcp",
                "https://m.example"
            ),
            None
        );
        // RFC 7636's example.
        assert!(pkce_ok(
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        ));
        assert!(!pkce_ok(
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            "wrong-verifier-wrong-verifier-wrong-verifier"
        ));
    }
}
