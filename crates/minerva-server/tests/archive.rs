//! Archived notes: out of search and the graph without being in the trash,
//! listed by `search` with `archived: true`, and brought back by unarchiving.
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
async fn search(router: &Router, token: &str, body: Value) -> Vec<String> {
    let (status, found) = api(router, "POST", "search", token, body).await;
    assert_eq!(status, 200);
    found
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn archived_notes_leave_search_and_the_graph() {
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
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let kept = note(
        &router,
        &owner,
        &folder,
        "Gardening plan",
        &["Tomatoes in May."],
    )
    .await;
    let old = note(
        &router,
        &owner,
        &folder,
        "Gardening 2024",
        &[&format!("See [the plan](#note/{kept}).")],
    )
    .await;
    assert!(search(&router, &owner, json!({"query": "gardening"}))
        .await
        .contains(&old));

    edit(&router, &owner, &old, |n| n["archived"] = json!(true)).await;
    let (_, archived) = api(&router, "GET", &format!("notes/{old}"), &owner, json!({})).await;
    assert_eq!(archived["archived"], true);
    assert_eq!(archived["trashed"], false, "archiving is not trashing");
    // A save that does not mention it (an older client) keeps it archived.
    edit(&router, &owner, &old, |n| {
        n.as_object_mut().unwrap().remove("archived");
        n["title"] = json!("Gardening 2024 (old)");
    })
    .await;
    let (_, still) = api(&router, "GET", &format!("notes/{old}"), &owner, json!({})).await;
    assert_eq!(still["archived"], true);

    // Out of search and the graph, but listed with `archived: true`.
    let found = search(&router, &owner, json!({"query": "gardening"})).await;
    assert_eq!(found, vec![kept.clone()]);
    let found = search(
        &router,
        &owner,
        json!({"query": "gardening", "archived": true}),
    )
    .await;
    assert_eq!(found, vec![old.clone()]);
    let (status, graph) = api(
        &router,
        "GET",
        &format!("workspaces/{workspace}/graph"),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(status, 200);
    let nodes: Vec<&str> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert!(nodes.contains(&kept.as_str()));
    assert!(!nodes.contains(&old.as_str()));

    // Bootstrap still carries it, marked, so the app can list the archive.
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let row = boot["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == old.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["archived"], true);

    // Unarchiving brings it back.
    edit(&router, &owner, &old, |n| n["archived"] = json!(false)).await;
    let found = search(&router, &owner, json!({"query": "gardening"})).await;
    assert!(found.contains(&old) && found.contains(&kept));
}
