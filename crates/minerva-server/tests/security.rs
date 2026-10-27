//! Abuse resistance: sign-in throttling and input size limits.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

const PASSWORD: &str = "test-password-1234";

async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
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
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn throttling_one_account_does_not_lock_out_others() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    api(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Victim","email":"victim@test.local","password":PASSWORD}),
    )
    .await;
    for _ in 0..10 {
        let (status, _) = api(
            &router,
            "POST",
            "login",
            "",
            json!({"email":"victim@test.local","password":"wrong-password-1"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, _) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"victim@test.local","password":PASSWORD}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "the attacked account is throttled"
    );
    let (status, other) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "other accounts can still sign in: {other}"
    );
    // Unknown accounts answer exactly like wrong passwords.
    let (status, unknown) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"nobody@test.local","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown["error"], "Invalid email or password");
}

#[tokio::test]
async fn render_rejects_oversized_input() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let token = common::member(&router, &root, "owner@test.local").await;
    let (status, _) = api(
        &router,
        "POST",
        "render",
        &token,
        json!({"markdown": "x".repeat(100_001)}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = api(
        &router,
        "POST",
        "render",
        &token,
        json!({"markdown": "x".repeat(100_000)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
