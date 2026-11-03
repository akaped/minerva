//! The graph of a workspace: only notes the reader can open, and only links
//! between them.
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
async fn graph_shows_only_what_the_reader_can_open() {
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
    let reader = common::account(&router, &root, "reader@test.local").await;
    let stranger = common::account(&router, &root, "stranger@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let wid = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let open = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, private) = api(
        &router,
        "POST",
        "folders",
        &owner,
        json!({"name":"Private","workspace_id":wid}),
    )
    .await;
    let note = |folder: String, title: &'static str, markdown: String| {
        let router = router.clone();
        let owner = owner.clone();
        async move {
            api(
                &router,
                "POST",
                "notes",
                &owner,
                json!({"folder_id":folder,"title":title,"markdown":markdown}),
            )
            .await
            .1["id"]
                .as_str()
                .unwrap()
                .to_string()
        }
    };
    let secret = note(
        private["id"].as_str().unwrap().into(),
        "Secret",
        "Hidden".into(),
    )
    .await;
    let b = note(
        open.clone(),
        "B",
        format!("Back to the [secret](#note/{secret}) #topic"),
    )
    .await;
    let a = note(
        open.clone(),
        "A",
        format!("Links to [B](#note/{b}) and [secret](#note/{secret})"),
    )
    .await;
    let (_, reader_boot) = api(&router, "GET", "bootstrap", &reader, json!({})).await;
    api(
        &router,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id":open,"user_id":reader_boot["user"]["id"],"role":"viewer"}),
    )
    .await;

    let path = format!("workspaces/{wid}/graph");
    let (status, full) = api(&router, "GET", &path, &owner, json!({})).await;
    assert_eq!(status, 200, "{full}");
    assert!(full["links"]
        .as_array()
        .unwrap()
        .contains(&json!([a, secret])));
    let (_, seen) = api(&router, "GET", &path, &reader, json!({})).await;
    let ids: Vec<&str> = seen["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&a.as_str()) && ids.contains(&b.as_str()));
    assert!(
        !ids.contains(&secret.as_str()),
        "a note the reader cannot open is not shown"
    );
    assert_eq!(seen["links"], json!([[a, b]]), "nor are links to it");
    let node_b = seen["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == b)
        .unwrap();
    assert_eq!(node_b["tags"], json!(["topic"]));
    let (status, _) = api(&router, "GET", &path, &stranger, json!({})).await;
    assert_eq!(status, 403);
}
