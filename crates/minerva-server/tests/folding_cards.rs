//! Folding kept in the note, and link previews for share-link guests.
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
async fn folding_kept_in_the_note_and_guest_previews() {
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
        json!({"folder_id": folder, "title": "Outline"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    let (_, mut full) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    full["blocks"] = json!([{"id":"p","markdown":"- Parent","x":0,"y":0},{"id":"c","markdown":"  - Child","x":0,"y":0},{"id":"l","markdown":"https://example.invalid/article","x":0,"y":0}]);
    api(&router, "PUT", &format!("notes/{nid}"), &owner, full).await;
    let folding = format!("notes/{nid}/folding");

    // Off by default; editors keep it in the note, known blocks only.
    let (_, before) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    assert!(before.get("folded").is_none());
    let (status, _) = api(
        &router,
        "PUT",
        &folding,
        &reader,
        json!({"shared": true, "folded": ["p"]}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, kept) = api(
        &router,
        "PUT",
        &folding,
        &owner,
        json!({"shared": true, "folded": ["p", "p", "gone"]}),
    )
    .await;
    assert_eq!(status, 200, "{kept}");
    assert_eq!(kept["folded"], json!(["p"]));
    let (_, seen) = api(&router, "GET", &format!("notes/{nid}"), &reader, json!({})).await;
    assert_eq!(
        seen["folded"],
        json!(["p"]),
        "everyone sees the same folding"
    );
    // Live editing keeps it.
    let (_, synced) = api(
        &router,
        "POST",
        &format!("notes/{nid}/sync"),
        &owner,
        json!({"update": ""}),
    )
    .await;
    assert_eq!(synced["note"]["folded"], json!(["p"]));
    // Off again: each device folds on its own.
    let (_, off) = api(&router, "PUT", &folding, &owner, json!({"shared": false})).await;
    assert!(off["folded"].is_null());
    let (status, _) = api(&router, "PUT", &folding, &owner, json!({})).await;
    assert_eq!(status, 400);

    // Guests preview only the addresses their note shows as cards.
    let (_, link) = api(
        &router,
        "POST",
        &format!("notes/{nid}/shares"),
        &owner,
        json!({"kind":"link","role":"viewer"}),
    )
    .await;
    let cookie = format!("minerva_share={}", link["token"].as_str().unwrap());
    let ask = |url: &str| {
        let router = router.clone();
        let cookie = cookie.clone();
        let uri = format!(
            "/api/v1/shared/link-preview?url={}",
            url::form_urlencoded::byte_serialize(url.as_bytes()).collect::<String>()
        );
        async move {
            router
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("cookie", cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };
    assert_eq!(
        ask("https://other.example/").await,
        403,
        "not an open proxy"
    );
    assert_ne!(
        ask("https://example.invalid/article").await,
        403,
        "the note's own card"
    );
    let anonymous = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/shared/link-preview?url=https%3A%2F%2Fexample.invalid%2Farticle")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status().as_u16(), 401);
}
