//! The web clipper: connecting with a one-time code and PKCE, a token that can
//! only list folders and add clipped notes, and the notes it adds.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
mod common;
use common::PASSWORD;

const REDIRECT: &str = "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/";
const VERIFIER: &str = "a-verifier-long-enough-for-pkce-0123456789abcdef";

async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(format!("/api/v1/{path}"))
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
/// Approves a connection as `session`, returning the code handed back.
async fn approve(router: &Router, session: &str) -> String {
    let (status, approved) = api(
        router,
        "POST",
        "clipper/approve",
        session,
        json!({"redirect_uri":REDIRECT,"state":"s1","challenge":challenge(VERIFIER),"browser":"chrome"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let back = url::Url::parse(approved["redirect"].as_str().unwrap()).unwrap();
    assert!(back.as_str().starts_with(REDIRECT), "{back}");
    let query: std::collections::HashMap<_, _> = back.query_pairs().into_owned().collect();
    assert_eq!(query["state"], "s1");
    query["code"].clone()
}
async fn collect(router: &Router, code: &str, verifier: &str) -> (StatusCode, Value) {
    api(
        router,
        "POST",
        "clipper/token",
        "",
        json!({"code":code,"verifier":verifier,"redirect_uri":REDIRECT}),
    )
    .await
}

#[tokio::test]
async fn the_clipper_connects_and_adds_notes_only() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (_, admin) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    let admin = admin["token"].as_str().unwrap().to_string();
    let owner = common::member(&router, &admin, "clipper@test.local").await;
    let other = common::member(&router, &admin, "other@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, other_boot) = api(&router, "GET", "bootstrap", &other, json!({})).await;
    let foreign = common::team_folder(&other_boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Only browser extensions' sign-in addresses, and only with a session.
    for bad in ["https://evil.example/", "http://abc.chromiumapp.org/"] {
        let (status, _) = api(
            &router,
            "POST",
            "clipper/approve",
            &owner,
            json!({"redirect_uri":bad,"state":"","challenge":challenge(VERIFIER)}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (_, script) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name":"script"}),
    )
    .await;
    let script = script["token"].as_str().unwrap();
    let (status, _) = api(
        &router,
        "POST",
        "clipper/approve",
        script,
        json!({"redirect_uri":REDIRECT,"state":"","challenge":challenge(VERIFIER)}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "API tokens cannot approve");
    let (status, _) = api(
        &router,
        "POST",
        "clipper/approve",
        &admin,
        json!({"redirect_uri":REDIRECT,"state":"","challenge":challenge(VERIFIER)}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "administrators do not clip");

    // A wrong verifier uses the code up.
    let code = approve(&router, &owner).await;
    assert_eq!(
        collect(
            &router,
            &code,
            "another-verifier-that-is-long-enough-000000"
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        collect(&router, &code, VERIFIER).await.0,
        StatusCode::BAD_REQUEST
    );
    let code = approve(&router, &owner).await;
    let (status, connected) = collect(&router, &code, VERIFIER).await;
    assert_eq!(status, StatusCode::OK, "{connected}");
    assert_eq!(connected["user"]["email"], "clipper@test.local");
    let clip = connected["token"].as_str().unwrap().to_string();
    assert_eq!(
        collect(&router, &code, VERIFIER).await.0,
        StatusCode::BAD_REQUEST,
        "once"
    );

    // It lists the folders its person can add to, and nothing else.
    let (status, folders) = api(&router, "GET", "clipper/folders", &clip, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<&str> = folders["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|w| w["folders"].as_array().unwrap())
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert!(
        listed.contains(&folder.as_str()) && !listed.contains(&foreign.as_str()),
        "{folders}"
    );
    for (method, path) in [
        ("GET", "bootstrap"),
        ("POST", "notes"),
        ("POST", "search"),
        ("GET", "api-tokens"),
        ("POST", "clipper/approve"),
    ] {
        let (status, _) = api(
            &router,
            method,
            path,
            &clip,
            json!({"folder_id":folder,"title":"x"}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }

    // A clip becomes a note with its source, in a folder its person may edit.
    let (status, saved) = api(
        &router,
        "POST",
        "clipper/clip",
        &clip,
        json!({"folder_id":folder,"title":"  An   article ","url":"https://example.com/a?b=1","markdown":"[example.com](https://example.com/a?b=1)\n\n# Heading\n\nBody text.","tags":["#Reading","not a tag"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (_, note) = api(
        &router,
        "GET",
        &format!("notes/{}", saved["id"].as_str().unwrap()),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(note["title"], "An article");
    assert_eq!(note["metadata"]["source"], "https://example.com/a?b=1");
    assert_eq!(note["tags"], json!(["reading"]));
    let text: Vec<&str> = note["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["markdown"].as_str().unwrap())
        .collect();
    assert_eq!(
        text,
        [
            "[example.com](https://example.com/a?b=1)",
            "# Heading",
            "Body text."
        ]
    );
    for (body, expected) in [
        (
            json!({"folder_id":foreign,"title":"x","url":"https://example.com"}),
            StatusCode::FORBIDDEN,
        ),
        (
            json!({"folder_id":folder,"title":"x","url":"javascript:alert(1)"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"folder_id":folder,"title":"x","url":"https://example.com","markdown":"x".repeat(200_001)}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (status, _) = api(&router, "POST", "clipper/clip", &clip, body).await;
        assert_eq!(status, expected);
    }

    // Connecting the same browser again replaces its token.
    let code = approve(&router, &owner).await;
    let (_, again) = collect(&router, &code, VERIFIER).await;
    let renewed = again["token"].as_str().unwrap();
    assert_eq!(
        api(&router, "GET", "clipper/folders", &clip, json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(&router, "GET", "clipper/folders", renewed, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    let (_, tokens) = api(&router, "GET", "api-tokens", &owner, json!({})).await;
    let clippers: Vec<_> = tokens
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["access"] == "clipper")
        .collect();
    assert_eq!(clippers.len(), 1, "{tokens}");
    assert_eq!(clippers[0]["name"], "Web clipper (Chrome)");

    // Disconnecting revokes it; a session cannot use that route.
    assert_eq!(
        api(&router, "DELETE", "clipper/token", &owner, json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(&router, "DELETE", "clipper/token", renewed, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        api(&router, "GET", "clipper/folders", renewed, json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
