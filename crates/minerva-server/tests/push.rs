//! Push notifications: the server key, subscribing and unsubscribing.
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
async fn devices_subscribe_to_reminder_notifications() {
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
    let (status, info) = api(&router, "GET", "push", &owner, json!({})).await;
    assert_eq!(status, 200);
    // An uncompressed P-256 public key, URL-safe base64: 87 characters.
    assert_eq!(info["key"].as_str().unwrap().len(), 87, "{info}");
    assert_eq!(info["subscriptions"], 0);

    let key = format!("B{}", "A".repeat(86));
    let auth = "AAAAAAAAAAAAAAAAAAAAAA";
    let endpoint = "https://fcm.googleapis.com/fcm/send/abc123";
    for (bad, why) in [
        (
            json!({"endpoint": "https://evil.example/push", "keys": {"p256dh": key, "auth": auth}}),
            "unknown service",
        ),
        (
            json!({"endpoint": "http://fcm.googleapis.com/fcm/send/x", "keys": {"p256dh": key, "auth": auth}}),
            "not https",
        ),
        (
            json!({"endpoint": endpoint, "keys": {"p256dh": "short", "auth": auth}}),
            "bad key",
        ),
    ] {
        let (status, _) = api(&router, "POST", "push/subscribe", &owner, bad).await;
        assert_eq!(status, 400, "{why}");
    }
    let (status, ok) = api(
        &router,
        "POST",
        "push/subscribe",
        &owner,
        json!({"endpoint": endpoint, "keys": {"p256dh": key, "auth": auth}}),
    )
    .await;
    assert_eq!(status, 200, "{ok}");
    let (_, info) = api(&router, "GET", "push", &owner, json!({})).await;
    assert_eq!(info["subscriptions"], 1);
    // The same address again replaces it.
    api(
        &router,
        "POST",
        "push/subscribe",
        &owner,
        json!({"endpoint": endpoint, "keys": {"p256dh": key, "auth": auth}}),
    )
    .await;
    let (_, info) = api(&router, "GET", "push", &owner, json!({})).await;
    assert_eq!(info["subscriptions"], 1);
    api(
        &router,
        "POST",
        "push/unsubscribe",
        &owner,
        json!({"endpoint": endpoint}),
    )
    .await;
    let (_, info) = api(&router, "GET", "push", &owner, json!({})).await;
    assert_eq!(info["subscriptions"], 0);
}
