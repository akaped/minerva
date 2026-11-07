use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;
async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    let r = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("authorization", format!("Bearer {token}"))
                .header("x-minerva-client", "1")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = r.status().as_u16();
    (
        status,
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap(),
    )
}
#[tokio::test]
async fn bootstrap_pages_cover_only_the_requested_workspace_without_duplicates() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap();
    let owner = common::member(&router, root, "owner@test.local").await;
    let stranger = common::account(&router, root, "stranger@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let ws = common::team_workspace(&boot)["id"].as_str().unwrap();
    let folder = common::team_folder(&boot)["id"].clone();
    for i in 0..5 {
        assert_eq!(
            api(
                &router,
                "POST",
                "notes",
                &owner,
                json!({"folder_id":folder,"title":format!("Page {i}")})
            )
            .await
            .0,
            200
        );
    }
    let (_, metadata) = api(&router, "GET", "bootstrap?notes=0", &owner, json!({})).await;
    assert_eq!(metadata["notes_paged"], true);
    assert!(metadata["notes"].as_array().unwrap().is_empty());
    assert!(!metadata["folders"].as_array().unwrap().is_empty());
    let mut seen = std::collections::HashSet::new();
    let mut after = String::new();
    loop {
        let path = format!("bootstrap/notes?workspace={ws}&limit=2&after={after}");
        let (status, page) = api(&router, "GET", &path, &owner, json!({})).await;
        assert_eq!(status, 200, "{page}");
        let notes = page["notes"].as_array().unwrap();
        assert!(notes.len() <= 2);
        for n in notes {
            assert_eq!(n["folder_id"], folder);
            assert!(seen.insert(n["id"].clone().to_string()));
        }
        match page["next"].as_str() {
            Some(cursor) => {
                assert!(cursor > after.as_str());
                after = cursor.into();
            }
            None => break,
        }
    }
    assert_eq!(seen.len(), 6);
    let path = format!("bootstrap/notes?workspace={ws}");
    assert_eq!(
        api(&router, "GET", &path, &stranger, json!({})).await.0,
        403
    );
    let (_, ownboot) = api(&router, "GET", "bootstrap", &stranger, json!({})).await;
    let (_, token) = api(
        &router,
        "POST",
        "api-tokens",
        &stranger,
        json!({"name":"Scoped", "workspace_id":ownboot["workspaces"][0]["id"]}),
    )
    .await;
    assert_eq!(
        api(
            &router,
            "GET",
            &path,
            token["token"].as_str().unwrap(),
            json!({})
        )
        .await
        .0,
        403
    );
    let (_, legacy) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    assert_eq!(legacy["notes"].as_array().unwrap().len(), 6);
    assert_eq!(legacy["notes_paged"], false);
}
