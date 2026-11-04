//! Editing one block where it is embedded: PUT notes/{id}/blocks/{block}.
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
async fn an_embedded_block_is_edited_where_it_is_shown() {
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
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, them) = api(&router, "GET", "bootstrap", &reader, json!({})).await;
    api(
        &router,
        "PUT",
        "workspace-access",
        &owner,
        json!({"workspace_id": workspace, "user_id": them["user"]["id"], "role": "viewer"}),
    )
    .await;
    let (_, n) = api(
        &router,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Source"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    let (_, mut full) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    full["blocks"] =
        json!([{"id":"a","markdown":"Alpha","x":0,"y":0},{"id":"b","markdown":"Beta","x":0,"y":0}]);
    api(&router, "PUT", &format!("notes/{nid}"), &owner, full).await;

    // The embed says who may edit, with the block's Markdown.
    let (_, shown) = api(
        &router,
        "POST",
        "embeds",
        &owner,
        json!({"items":[{"note": nid, "block": "b"}]}),
    )
    .await;
    assert_eq!(shown[0]["can_edit"], true, "{shown}");
    assert_eq!(shown[0]["blocks"][0]["markdown"], "Beta");
    let (_, seen) = api(
        &router,
        "POST",
        "embeds",
        &reader,
        json!({"items":[{"note": nid, "block": "b"}]}),
    )
    .await;
    assert_eq!(seen[0]["can_edit"], false);

    let path = format!("notes/{nid}/blocks/b");
    let (status, _) = api(
        &router,
        "PUT",
        &path,
        &reader,
        json!({"markdown": "No", "base": "Beta"}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = api(
        &router,
        "PUT",
        &path,
        &owner,
        json!({"markdown": "Stale", "base": "Something else"}),
    )
    .await;
    assert_eq!(status, 409, "a block changed meanwhile is not overwritten");
    let (status, saved) = api(
        &router,
        "PUT",
        &path,
        &owner,
        json!({"markdown": "Beta, revised", "base": "Beta"}),
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    let (_, after) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    assert_eq!(after["blocks"][1]["markdown"], "Beta, revised");
    assert_eq!(after["blocks"][0]["markdown"], "Alpha", "other blocks stay");
    let (status, _) = api(
        &router,
        "PUT",
        &format!("notes/{nid}/blocks/zz"),
        &owner,
        json!({"markdown": "x"}),
    )
    .await;
    assert_eq!(status, 400);
}
