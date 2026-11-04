//! Positions people arrange in the graph: kept per person and workspace,
//! returned with the graph, released one by one or all at once.
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
/// Creates a note with one block per entry of `blocks`; returns its ID.
async fn note(router: &Router, token: &str, folder: &str, title: &str, blocks: &[&str]) -> String {
    let (_, created) = api(
        router,
        "POST",
        "notes",
        token,
        json!({"folder_id": folder, "title": title}),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    edit(router, token, &id, |n| {
        n["blocks"] = json!(blocks
            .iter()
            .enumerate()
            .map(|(i, b)| json!({"id": format!("{id}-{i}"), "markdown": b, "x": 0, "y": 0}))
            .collect::<Vec<_>>());
    })
    .await;
    id
}
async fn edit(router: &Router, token: &str, id: &str, change: impl FnOnce(&mut Value)) {
    let path = format!("notes/{id}");
    let (_, mut n) = api(router, "GET", &path, token, json!({})).await;
    change(&mut n);
    let (status, _) = api(router, "PUT", &path, token, n).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn arranged_positions_are_kept_per_person() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, session) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email": "admin@test.local", "password": common::PASSWORD}),
    )
    .await;
    let root = session["token"].as_str().unwrap().to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let other = common::member(&router, &root, "other@test.local").await;
    let stranger = common::account(&router, &root, "stranger@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let a = note(&router, &owner, &folder, "A", &["one"]).await;
    let b = note(&router, &owner, &folder, "B", &[&format!("[A](#note/{a})")]).await;
    let (_, them) = api(&router, "GET", "bootstrap", &other, json!({})).await;
    let (status, _) = api(
        &router,
        "PUT",
        "workspace-access",
        &owner,
        json!({"workspace_id": workspace, "user_id": them["user"]["id"], "role": "viewer"}),
    )
    .await;
    assert_eq!(status, 200);
    let graph = format!("workspaces/{workspace}/graph");
    let positions = format!("{graph}/positions");

    let (status, saved) = api(
        &router,
        "PUT",
        &positions,
        &owner,
        json!({"positions": {a.clone(): [10.5, -20], "#tag": [1, 2]}}),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    let (_, g) = api(&router, "GET", &graph, &owner, json!({})).await;
    assert_eq!(g["positions"][&a], json!([10.5, -20.0]));
    assert_eq!(g["positions"]["#tag"], json!([1.0, 2.0]));

    // Another member's graph is theirs.
    let (_, theirs) = api(&router, "GET", &graph, &other, json!({})).await;
    assert_eq!(theirs["positions"], json!({}));
    // Someone outside the workspace keeps nothing there.
    let (status, _) = api(
        &router,
        "PUT",
        &positions,
        &stranger,
        json!({"positions": {a.clone(): [0, 0]}}),
    )
    .await;
    assert_eq!(status, 403);
    // Nonsense is refused.
    for bad in [
        json!({"positions": {a.clone(): [1]}}),
        json!({"positions": {a.clone(): "x"}}),
        json!({}),
    ] {
        let (status, _) = api(&router, "PUT", &positions, &owner, bad).await;
        assert_eq!(status, 400);
    }

    // Moving one keeps the rest; null releases one; reset releases all.
    api(
        &router,
        "PUT",
        &positions,
        &owner,
        json!({"positions": {b.clone(): [3, 4], "#tag": null}}),
    )
    .await;
    let (_, g) = api(&router, "GET", &graph, &owner, json!({})).await;
    assert_eq!(g["positions"].as_object().unwrap().len(), 2);
    assert!(g["positions"]["#tag"].is_null());
    let (status, _) = api(&router, "PUT", &positions, &owner, json!({"reset": true})).await;
    assert_eq!(status, 200);
    let (_, g) = api(&router, "GET", &graph, &owner, json!({})).await;
    assert_eq!(g["positions"], json!({}));
}
