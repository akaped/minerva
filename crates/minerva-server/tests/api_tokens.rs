//! Personal API tokens: they act as their owner, expire and can be revoked, and
//! cannot manage the account.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("content-type", "application/json")
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn tokens_act_as_their_owner_and_can_be_revoked() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let other = common::account(&router, &root, "other@test.local").await;

    // Administrators cannot have tokens; names and expiry are checked.
    let (status, _) = api(
        &router,
        "POST",
        "api-tokens",
        &root,
        json!({"name":"Admin"}),
    )
    .await;
    assert!(status == 400 || status == 403, "{status}");
    for bad in [
        json!({"name":""}),
        json!({"name":"x","expires_days":0}),
        json!({"name":"x","expires_days":400}),
    ] {
        let (status, _) = api(&router, "POST", "api-tokens", &owner, bad.clone()).await;
        assert_eq!(status, 400, "{bad}");
    }
    let (status, created) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name":"Laptop script","expires_days":30}),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let token = created["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("mnv_api_v1_"));
    let (_, listed) = api(&router, "GET", "api-tokens", &owner, json!({})).await;
    assert_eq!(listed[0]["name"], "Laptop script");
    assert!(listed[0].get("token").is_none() && listed[0].get("token_hash").is_none());
    let (_, others) = api(&router, "GET", "api-tokens", &other, json!({})).await;
    assert!(others.as_array().unwrap().is_empty());

    // The token reads and writes as its owner.
    let (status, boot) = api(&router, "GET", "bootstrap", &token, json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(boot["user"]["email"], "owner@test.local");
    let folder = common::team_folder(&boot)["id"].clone();
    let (status, note) = api(
        &router,
        "POST",
        "notes",
        &token,
        json!({"folder_id":folder,"title":"From a script"}),
    )
    .await;
    assert_eq!(status, 200, "{note}");
    let (status, found) = api(&router, "POST", "search", &token, json!({"query":"script"})).await;
    assert_eq!(status, 200);
    assert!(found.to_string().contains("From a script"));

    // It cannot manage the account.
    for (method, path) in [
        ("GET", "api-tokens"),
        ("POST", "api-tokens"),
        ("GET", "mcp-credentials"),
        ("POST", "account/password"),
        ("GET", "account/2fa"),
        ("POST", "logout"),
    ] {
        let (status, _) = api(&router, method, path, &token, json!({"name":"x"})).await;
        assert_eq!(status, 403, "{method} {path}");
    }

    // Someone else cannot revoke it; its owner can, and then it stops working.
    let path = format!("api-tokens/{}", created["id"].as_str().unwrap());
    let (status, _) = api(&router, "DELETE", &path, &other, json!({})).await;
    assert_eq!(status, 404);
    let (status, _) = api(&router, "DELETE", &path, &owner, json!({})).await;
    assert_eq!(status, 200);
    let (status, _) = api(&router, "GET", "bootstrap", &token, json!({})).await;
    assert_eq!(status, 401);
    let (status, _) = api(&router, "GET", "bootstrap", "mnv_api_v1_made-up", json!({})).await;
    assert_eq!(status, 401);
}
