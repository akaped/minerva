//! OAuth for MCP clients: discovery, registration, consent with PKCE, tokens
//! that are MCP credentials, refresh rotation and revocation.
use axum::{body::Body, http::Request, Router};
use base64::Engine;
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
mod common;

async fn raw(
    r: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: String,
) -> (u16, Vec<(String, String)>, String) {
    let mut req = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let response = r
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let heads = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, heads, String::from_utf8_lossy(&bytes).into_owned())
}
async fn api(r: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    let auth = format!("Bearer {token}");
    let (status, _, text) = raw(
        r,
        method,
        &format!("/api/v1/{path}"),
        &[
            ("content-type", "application/json"),
            ("x-minerva-client", "1"),
            ("authorization", &auth),
        ],
        body.to_string(),
    )
    .await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}
fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}
fn header<'a>(heads: &'a [(String, String)], name: &str) -> Option<&'a str> {
    heads
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}
async fn mcp(r: &Router, wid: &str, token: &str) -> (u16, Vec<(String, String)>, Value) {
    let auth = format!("Bearer {token}");
    let (status, heads, text) = raw(
        r,
        "POST",
        &format!("/api/workspaces/{wid}/mcp"),
        &[
            ("content-type", "application/json"),
            ("accept", "application/json, text/event-stream"),
            ("authorization", &auth),
        ],
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
    )
    .await;
    (
        status,
        heads,
        serde_json::from_str(&text).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn an_mcp_client_connects_with_oauth() {
    // SAFETY: set before the server reads it, in this test's own process.
    unsafe {
        std::env::set_var("MINERVA_MCP_OAUTH", "1");
        std::env::set_var("MINERVA_PUBLIC_URL", "https://notes.example");
    }
    let r = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&r, &root, "owner@test.local").await;
    let (_, boot) = api(&r, "GET", "bootstrap", &owner, json!({})).await;
    let wid = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resource = format!("https://notes.example/api/workspaces/{wid}/mcp");

    // Discovery: the MCP endpoint points to its metadata, which names the server.
    let (status, heads, _) = mcp(&r, &wid, "nothing").await;
    assert_eq!(status, 401);
    let challenge = header(&heads, "www-authenticate").unwrap();
    assert!(
        challenge.contains(&format!(
            "/.well-known/oauth-protected-resource/api/workspaces/{wid}/mcp"
        )),
        "{challenge}"
    );
    let (_, _, meta) = raw(
        &r,
        "GET",
        &format!("/.well-known/oauth-protected-resource/api/workspaces/{wid}/mcp"),
        &[],
        String::new(),
    )
    .await;
    let meta: Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(meta["resource"], resource.as_str());
    let (_, _, server) = raw(
        &r,
        "GET",
        "/.well-known/oauth-authorization-server",
        &[],
        String::new(),
    )
    .await;
    let server: Value = serde_json::from_str(&server).unwrap();
    assert_eq!(server["code_challenge_methods_supported"], json!(["S256"]));

    // Registration: public clients with safe redirects.
    let (status, _, _) = raw(
        &r,
        "POST",
        "/oauth/register",
        &[("content-type", "application/json")],
        json!({"client_name":"Bad","redirect_uris":["http://evil.example/cb"]}).to_string(),
    )
    .await;
    assert_eq!(status, 400);
    let redirect = "http://localhost:7777/callback";
    let (status, _, client) = raw(
        &r,
        "POST",
        "/oauth/register",
        &[("content-type", "application/json")],
        json!({"client_name":"Desk AI","redirect_uris":[redirect]}).to_string(),
    )
    .await;
    assert_eq!(status, 201, "{client}");
    let client_id = serde_json::from_str::<Value>(&client).unwrap()["client_id"]
        .as_str()
        .unwrap()
        .to_string();

    let verifier = "a-very-long-random-verifier-string-0123456789abcdef";
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let params = [
        ("response_type", "code"),
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", "xyz"),
        ("scope", "mcp:read mcp:write"),
        ("resource", resource.as_str()),
    ];
    let query = form(&params);

    // Signed out: the page asks to sign in. Signed in: consent.
    let (status, _, page) = raw(
        &r,
        "GET",
        &format!("/oauth/authorize?{query}"),
        &[],
        String::new(),
    )
    .await;
    assert_eq!(status, 200);
    assert!(page.contains("Sign in"), "{page}");
    let cookie = format!("minerva_session={owner}");
    let (status, heads, page) = raw(
        &r,
        "GET",
        &format!("/oauth/authorize?{query}"),
        &[("cookie", &cookie)],
        String::new(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(header(&heads, "x-frame-options"), Some("DENY"));
    assert!(
        page.contains("Desk AI") && page.contains("localhost"),
        "{page}"
    );
    let consent = page
        .split("name=\"consent\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();

    // Without the page's key, nothing is approved.
    let mut answer: Vec<(&str, &str)> = params.to_vec();
    answer.extend([
        ("workspace", wid.as_str()),
        ("access", "read_write"),
        ("decision", "allow"),
    ]);
    let (status, _, _) = raw(
        &r,
        "POST",
        "/oauth/authorize",
        &[
            ("cookie", &cookie),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&answer),
    )
    .await;
    assert_eq!(status, 403);
    answer.push(("consent", consent.as_str()));
    let (status, heads, _) = raw(
        &r,
        "POST",
        "/oauth/authorize",
        &[
            ("cookie", &cookie),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&answer),
    )
    .await;
    assert_eq!(status, 302);
    let location = url::Url::parse(header(&heads, "location").unwrap()).unwrap();
    assert!(location.as_str().starts_with(redirect));
    let got: std::collections::HashMap<String, String> =
        location.query_pairs().into_owned().collect();
    assert_eq!(got["state"], "xyz");
    let code = got["code"].clone();

    // The code, with the right verifier, once.
    let token_url = "/oauth/token";
    let content = [("content-type", "application/x-www-form-urlencoded")];
    let (status, _, said) = raw(
        &r,
        "POST",
        token_url,
        &content,
        form(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", redirect),
            ("client_id", &client_id),
            (
                "code_verifier",
                "wrong-wrong-wrong-wrong-wrong-wrong-wrong-wrong",
            ),
        ]),
    )
    .await;
    assert_eq!(status, 400, "{said}");
    // A wrong verifier used the code up; start again for a fresh one.
    let (_, _, page) = raw(
        &r,
        "GET",
        &format!("/oauth/authorize?{query}"),
        &[("cookie", &cookie)],
        String::new(),
    )
    .await;
    let consent = page
        .split("name=\"consent\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let mut answer: Vec<(&str, &str)> = params.to_vec();
    answer.extend([
        ("workspace", wid.as_str()),
        ("access", "read_write"),
        ("decision", "allow"),
        ("consent", consent.as_str()),
    ]);
    let (_, heads, _) = raw(
        &r,
        "POST",
        "/oauth/authorize",
        &[
            ("cookie", &cookie),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&answer),
    )
    .await;
    let location = url::Url::parse(header(&heads, "location").unwrap()).unwrap();
    let code: String = location
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned();
    let exchange = form(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", redirect),
        ("client_id", &client_id),
        ("code_verifier", verifier),
    ]);
    let (status, heads, tokens) = raw(&r, "POST", token_url, &content, exchange.clone()).await;
    assert_eq!(status, 200, "{tokens}");
    assert_eq!(header(&heads, "cache-control"), Some("no-store"));
    let tokens: Value = serde_json::from_str(&tokens).unwrap();
    assert_eq!(tokens["scope"], "mcp:read mcp:write");
    let access = tokens["access_token"].as_str().unwrap().to_string();
    let (status, _, _) = raw(&r, "POST", token_url, &content, exchange).await;
    assert_eq!(status, 400, "a code works once");

    // The token is an MCP credential with write tools, listed in connections.
    let (status, _, listed) = mcp(&r, &wid, &access).await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 12);
    let (_, creds) = api(&r, "GET", "mcp-credentials", &owner, json!({})).await;
    let cred = creds
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Desk AI (OAuth)")
        .unwrap()
        .clone();
    assert_eq!(cred["access"], "read_write");

    // Refreshing rotates both tokens.
    let refresh = tokens["refresh_token"].as_str().unwrap().to_string();
    let renew = form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh),
        ("client_id", &client_id),
    ]);
    let (status, _, again) = raw(&r, "POST", token_url, &content, renew.clone()).await;
    assert_eq!(status, 200, "{again}");
    let again: Value = serde_json::from_str(&again).unwrap();
    assert_ne!(again["access_token"], tokens["access_token"]);
    assert_eq!(
        mcp(&r, &wid, &access).await.0,
        401,
        "the old access token ends"
    );
    assert_eq!(
        mcp(&r, &wid, again["access_token"].as_str().unwrap())
            .await
            .0,
        200
    );
    assert_eq!(
        raw(&r, "POST", token_url, &content, renew).await.0,
        400,
        "a refresh token works once"
    );

    // Revoking the connection ends refreshing too.
    let (status, _) = api(
        &r,
        "DELETE",
        &format!("mcp-credentials/{}", cred["id"].as_str().unwrap()),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(status, 200);
    let next = again["refresh_token"].as_str().unwrap();
    let (status, _, said) = raw(
        &r,
        "POST",
        token_url,
        &content,
        form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", next),
            ("client_id", &client_id),
        ]),
    )
    .await;
    assert_eq!(status, 400, "{said}");

    // Denying sends the client back with an error.
    let (_, _, page) = raw(
        &r,
        "GET",
        &format!("/oauth/authorize?{query}"),
        &[("cookie", &cookie)],
        String::new(),
    )
    .await;
    let consent = page
        .split("name=\"consent\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let mut answer: Vec<(&str, &str)> = params.to_vec();
    answer.extend([
        ("workspace", wid.as_str()),
        ("decision", "deny"),
        ("consent", consent.as_str()),
    ]);
    let (_, heads, _) = raw(
        &r,
        "POST",
        "/oauth/authorize",
        &[
            ("cookie", &cookie),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&answer),
    )
    .await;
    assert!(header(&heads, "location")
        .unwrap()
        .contains("error=access_denied"));
}
