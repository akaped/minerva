//! Collaborators see which block each person is on, and only one of them can
//! have a block open for editing at a time.
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

async fn post(router: &Router, uri: &str, token: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("x-minerva-client", "1")
                .header("content-type", "application/json")
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
async fn the_first_client_to_open_a_block_keeps_it() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = post(
        &router,
        "/api/v1/login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let boot = {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/bootstrap")
                    .header("authorization", format!("Bearer {owner}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    };
    let sync = format!(
        "/api/v1/notes/{}/sync",
        common::welcome(&boot)["id"].as_str().unwrap()
    );
    let at = |client: &str, block: Option<&str>, editing: bool| json!({"update":"","client_id":client,"active_block":block,"editing":editing});

    // The first window opens block b1.
    let (status, first) = post(&router, &sync, &owner, at("one", Some("b1"), true)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(first["denied_block"].is_null());
    // A second window asking for the same block is turned down and shown the holder.
    let (_, second) = post(&router, &sync, &owner, at("two", Some("b1"), true)).await;
    assert_eq!(second["denied_block"], "b1");
    let holder = second["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["client_id"] == "one")
        .unwrap();
    assert_eq!(holder["editing"], true);
    assert_eq!(holder["active_block"], "b1");
    let me = second["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["client_id"] == "two")
        .unwrap();
    assert_eq!(me["editing"], false);
    // Another block is free.
    let (_, other) = post(&router, &sync, &owner, at("two", Some("b2"), true)).await;
    assert!(other["denied_block"].is_null());
    // Once the holder leaves, the block can be taken.
    post(
        &router,
        &sync,
        &owner,
        json!({"update":"","client_id":"one","leave":true}),
    )
    .await;
    let (_, taken) = post(&router, &sync, &owner, at("two", Some("b1"), true)).await;
    assert!(taken["denied_block"].is_null());
    assert!(taken["peers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["client_id"] != "one"));
}
