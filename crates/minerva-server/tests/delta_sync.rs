//! Note sync sends a client only what it lacks, when it says what it has.
use axum::{body::Body, http::Request, Router};
use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
use yrs::{updates::decoder::Decode, Doc, GetString, ReadTxn, Text, Transact, Update};
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
fn apply(doc: &Doc, update: &str) {
    let bytes = STANDARD.decode(update).unwrap();
    doc.transact_mut()
        .apply_update(Update::decode_v1(&bytes).unwrap())
        .unwrap();
}
fn vector(doc: &Doc) -> String {
    use yrs::updates::encoder::Encode;
    STANDARD.encode(doc.transact().state_vector().encode_v1())
}

#[tokio::test]
async fn sync_sends_only_what_the_client_lacks() {
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
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, n) = api(
        &router,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Long"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    let (_, mut full) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    full["blocks"] = json!((0..200).map(|i| json!({"id": format!("b{i}"), "markdown": format!("Paragraph {i} with a fair amount of text in it."), "x": 0, "y": 0})).collect::<Vec<_>>());
    api(&router, "PUT", &format!("notes/{nid}"), &owner, full).await;
    let path = format!("notes/{nid}/sync");

    // A first sync without a vector: everything.
    let (status, first) = api(&router, "POST", &path, &owner, json!({"update": ""})).await;
    assert_eq!(status, 200, "{first}");
    let everything = first["update"].as_str().unwrap().len();
    let mine = Doc::new();
    apply(&mine, first["update"].as_str().unwrap());

    // Another client changes one block.
    let theirs = Doc::new();
    apply(&theirs, first["update"].as_str().unwrap());
    let before = theirs.transact().state_vector();
    {
        let text = theirs.get_or_insert_text("block:b7");
        let mut txn = theirs.transact_mut();
        text.insert(&mut txn, 0, "Edited: ");
    }
    let change = STANDARD.encode(theirs.transact().encode_state_as_update_v1(&before));
    let (status, _) = api(&router, "POST", &path, &owner, json!({"update": change})).await;
    assert_eq!(status, 200);

    // Saying what it has, the first client gets a small reply that catches it up.
    let (status, reply) = api(
        &router,
        "POST",
        &path,
        &owner,
        json!({"update": "", "state_vector": vector(&mine)}),
    )
    .await;
    assert_eq!(status, 200, "{reply}");
    let delta = reply["update"].as_str().unwrap();
    assert!(
        delta.len() * 20 < everything,
        "{} vs {everything}",
        delta.len()
    );
    apply(&mine, delta);
    let text = mine.get_or_insert_text("block:b7");
    assert!(text.get_string(&mine.transact()).starts_with("Edited: "));
    assert_eq!(
        reply["note"]["blocks"][7]["markdown"].as_str().unwrap(),
        text.get_string(&mine.transact())
    );

    // Nonsense is refused; up to date, little comes back (the deletions it already knows).
    let (status, _) = api(
        &router,
        "POST",
        &path,
        &owner,
        json!({"update": "", "state_vector": "%%%"}),
    )
    .await;
    assert_eq!(status, 400);
    let (_, idle) = api(
        &router,
        "POST",
        &path,
        &owner,
        json!({"update": "", "state_vector": vector(&mine)}),
    )
    .await;
    assert!(idle["update"].as_str().unwrap().len() * 20 < everything);
}
