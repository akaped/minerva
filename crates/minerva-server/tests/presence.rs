//! Collaborators see which block each person is on, and only one of them can
//! have a block open for editing at a time.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
use yrs::{updates::decoder::Decode, Doc, ReadTxn, StateVector, Text, Transact, Update};
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

    let (_, initial) = post(&router, &sync, &owner, at("one", None, false)).await;
    let block = initial["note"]["blocks"][0]["id"].as_str().unwrap();
    // The first window opens the block.
    let (status, first) = post(&router, &sync, &owner, at("one", Some(block), true)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(first["denied_block"].is_null());
    // A second window asking for the same block is turned down and shown the holder.
    let (_, second) = post(&router, &sync, &owner, at("two", Some(block), true)).await;
    assert_eq!(second["denied_block"], block);
    let holder = second["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["client_id"] == "one")
        .unwrap();
    assert_eq!(holder["editing"], true);
    assert_eq!(holder["active_block"], block);
    let me = second["peers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["client_id"] == "two")
        .unwrap();
    assert_eq!(me["editing"], false);
    // Even a client with a stale editor open cannot write through the lock.
    let doc = Doc::new();
    let state = STANDARD
        .decode(initial["update"].as_str().unwrap())
        .unwrap();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&state).unwrap())
        .unwrap();
    let text = doc.get_or_insert_text(format!("block:{block}"));
    text.insert(&mut doc.transact_mut(), 0, "second editor ");
    let update = STANDARD.encode(doc.transact().encode_diff_v1(&StateVector::default()));
    let mut changed = at("two", Some(block), true);
    changed["update"] = json!(update);
    let (status, _) = post(&router, &sync, &owner, changed.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, held) = post(&router, &sync, &owner, at("one", Some(block), true)).await;
    assert_eq!(
        held["note"]["blocks"][0]["markdown"],
        initial["note"]["blocks"][0]["markdown"]
    );
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
    let (_, taken) = post(&router, &sync, &owner, at("two", Some(block), true)).await;
    assert!(taken["denied_block"].is_null());
    assert!(taken["peers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["client_id"] != "one"));
    let (status, saved) = post(&router, &sync, &owner, changed).await;
    assert_eq!(status, StatusCode::OK);
    assert!(saved["note"]["blocks"][0]["markdown"]
        .as_str()
        .unwrap()
        .starts_with("second editor "));
}
