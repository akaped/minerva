//! Full-text search: ranking, prefixes, accents, access, trash, edits, deletion,
//! snippets, tags, workspace filter, and query text that FTS5 would parse.
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
async fn search(router: &Router, token: &str, query: &str) -> Vec<Value> {
    let (status, found) = api(router, "POST", "search", token, json!({"query": query})).await;
    assert_eq!(status, 200, "{query}");
    found.as_array().unwrap().clone()
}
fn ids(found: &[Value]) -> Vec<&str> {
    found.iter().map(|n| n["id"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn ranked_accent_insensitive_search_follows_access_and_edits() {
    let path = std::env::temp_dir().join(format!("minerva-search-{}.db", uuid::Uuid::new_v4()));
    let router =
        app(Store::open(path.to_str().unwrap(), "admin@test.local", common::PASSWORD).unwrap());
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
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    let titled = note(
        &router,
        &owner,
        &folder,
        "Photosynthesis basics",
        &["Plants turn light into sugar."],
    )
    .await;
    let mentioned = note(
        &router,
        &owner,
        &folder,
        "Biology diary",
        &["Today we covered **photosynthesis** in [class](https://example.com/class)."],
    )
    .await;
    let italian = note(
        &router,
        &owner,
        &folder,
        "Perché studiare",
        &["La ragione è semplice. #exam"],
    )
    .await;
    let (_, other_boot) = api(&router, "GET", "bootstrap", &other, json!({})).await;
    let other_folder = common::team_folder(&other_boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let private = note(
        &router,
        &other,
        &other_folder,
        "Photosynthesis secrets",
        &["Mine alone."],
    )
    .await;

    // Title matches rank above text matches, and text matches come with a snippet.
    let found = search(&router, &owner, "photosynthesis").await;
    assert_eq!(ids(&found), [titled.as_str(), mentioned.as_str()]);
    let snippet = found[1]["snippet"].as_str().unwrap();
    assert!(snippet.contains("\u{1}photosynthesis\u{2}"), "{snippet}");
    assert!(
        !snippet.contains("example.com"),
        "link targets are not indexed: {snippet}"
    );
    // Prefixes while typing, accents and case ignored, every word required.
    assert_eq!(ids(&search(&router, &owner, "Photosyn").await).len(), 2);
    assert_eq!(
        ids(&search(&router, &owner, "perche").await),
        [italian.as_str()]
    );
    assert_eq!(
        ids(&search(&router, &owner, "ragione semp").await),
        [italian.as_str()]
    );
    assert!(search(&router, &owner, "ragione photosynthesis")
        .await
        .is_empty());
    // Another member's note is never found, by anyone else.
    assert!(!ids(&search(&router, &owner, "secrets").await).contains(&private.as_str()));
    assert_eq!(
        ids(&search(&router, &other, "secrets").await),
        [private.as_str()]
    );
    // Tags, through `#` and as words.
    assert_eq!(
        ids(&search(&router, &owner, "#exa").await),
        [italian.as_str()]
    );
    assert_eq!(
        ids(&search(&router, &owner, "exam").await),
        [italian.as_str()]
    );
    // The workspace filter.
    let (_, none) = api(
        &router,
        "POST",
        "search",
        &owner,
        json!({"query": "photosynthesis", "workspace_id": "elsewhere"}),
    )
    .await;
    assert!(none.as_array().unwrap().is_empty());
    let (_, here) = api(
        &router,
        "POST",
        "search",
        &owner,
        json!({"query": "photosynthesis", "workspace_id": workspace}),
    )
    .await;
    assert_eq!(here.as_array().unwrap().len(), 2);
    // Query text is words, never FTS5 syntax.
    for odd in [
        "\"",
        "*",
        "AND",
        "(",
        "title:plants",
        "NEAR(a b)",
        "a OR",
        "-",
        "^x",
        "🧭",
    ] {
        search(&router, &owner, odd).await;
    }

    // Edits and renames are found by their new text only.
    edit(&router, &owner, &mentioned, |n| {
        n["blocks"][0]["markdown"] = json!("Nothing here about plants any more.");
    })
    .await;
    assert_eq!(
        ids(&search(&router, &owner, "photosynthesis").await),
        [titled.as_str()]
    );
    assert_eq!(ids(&search(&router, &owner, "plants").await).len(), 2);
    edit(&router, &owner, &titled, |n| {
        n["title"] = json!("Light reactions")
    })
    .await;
    assert!(search(&router, &owner, "photosynthesis").await.is_empty());
    assert_eq!(
        ids(&search(&router, &owner, "reactions").await),
        [titled.as_str()]
    );
    // The trash hides a note; deleting it removes it.
    edit(&router, &owner, &titled, |n| n["trashed"] = json!(true)).await;
    assert!(search(&router, &owner, "reactions").await.is_empty());
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("notes/{titled}"),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(status, 200);
    edit(&router, &owner, &mentioned, |n| {
        n["title"] = json!("Light reactions too")
    })
    .await;
    assert_eq!(
        ids(&search(&router, &owner, "reactions").await),
        [mentioned.as_str()]
    );

    // The index survives a restart.
    drop(router);
    let router =
        app(Store::open(path.to_str().unwrap(), "admin@test.local", common::PASSWORD).unwrap());
    assert_eq!(
        ids(&search(&router, &owner, "reactions").await),
        [mentioned.as_str()]
    );
    assert_eq!(
        ids(&search(&router, &owner, "perche").await),
        [italian.as_str()]
    );
}
