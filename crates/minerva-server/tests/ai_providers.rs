//! Editing AI provider profiles: administrators only, the stored key kept unless
//! replaced, and never reused for another server without being entered again.
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

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/api/v1/{path}"))
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn login(app: &Router, email: &str) -> String {
    let (status, v) = request(
        app,
        "POST",
        "login",
        "",
        json!({"email":email,"password":"test-password-1234"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["token"].as_str().unwrap().into()
}

#[tokio::test]
async fn providers_can_be_edited_without_exposing_their_key() {
    let path = std::env::temp_dir().join(format!("minerva-ai-{}.db", uuid::Uuid::new_v4()));
    let app = app(Store::open(
        path.to_str().unwrap(),
        "admin@test.local",
        "test-password-1234",
    )
    .unwrap());
    let root = login(&app, "admin@test.local").await;
    let base = json!({"kind":"openai","name":"Team","endpoint":"https://api.openai.com/v1","model":"gpt-a"});
    let mut create = base.clone();
    create["api_key"] = json!("sk-original");
    let (status, created) = request(&app, "POST", "ai/providers", &root, create).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    assert_eq!(created["max_tokens"], 4096, "the default answer limit");
    let mut too_small = base.clone();
    too_small["api_key"] = json!("sk-test");
    too_small["max_tokens"] = json!(10);
    assert_eq!(
        request(&app, "POST", "ai/providers", &root, too_small)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let id = created["id"].as_str().unwrap();
    let route = format!("ai/providers/{id}");

    // Name, model, and path on the same server: the stored key is kept.
    let mut rename = base.clone();
    rename["name"] = json!("Research");
    rename["model"] = json!("gpt-b");
    rename["endpoint"] = json!("https://api.openai.com/v2/");
    rename["api_key"] = json!("");
    let (status, saved) = request(&app, "PATCH", &route, &root, rename).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (_, list) = request(&app, "GET", "ai/providers", &root, json!({})).await;
    assert_eq!(list[0]["name"], "Research");
    assert_eq!(list[0]["model"], "gpt-b");
    assert_eq!(list[0]["endpoint"], "https://api.openai.com/v2");
    assert!(list[0].get("api_key").is_none() && !list.to_string().contains("sk-original"));

    // Another server: only with the key entered again.
    let mut elsewhere = base.clone();
    elsewhere["endpoint"] = json!("https://collector.example/v1");
    let (status, error) = request(&app, "PATCH", &route, &root, elsewhere.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("API key again"));
    let (_, list) = request(&app, "GET", "ai/providers", &root, json!({})).await;
    assert_eq!(list[0]["endpoint"], "https://api.openai.com/v2");
    elsewhere["api_key"] = json!("sk-new");
    let (status, _) = request(&app, "PATCH", &route, &root, elsewhere).await;
    assert_eq!(status, StatusCode::OK);

    // Web search: only where the provider has its own search tool.
    let mut search = base.clone();
    search["web_search"] = json!(true);
    let (status, saved) = request(&app, "PATCH", &route, &root, search.clone()).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "needs the key again for the old server: {saved}"
    );
    search["endpoint"] = json!("https://collector.example/v1");
    let (status, saved) = request(&app, "PATCH", &route, &root, search.clone()).await;
    assert_eq!(
        (status, saved["web_search"].as_bool()),
        (StatusCode::OK, Some(true)),
        "{saved}"
    );
    let (_, list) = request(&app, "GET", "ai/providers", &root, json!({})).await;
    assert_eq!(list[0]["web_search"], true);
    search["kind"] = json!("compatible");
    let (status, error) = request(&app, "PATCH", &route, &root, search).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("Web search"));

    // Invalid values and unknown profiles are refused; members cannot edit.
    let mut bad = base.clone();
    bad["kind"] = json!("mystery");
    assert_eq!(
        request(&app, "PATCH", &route, &root, bad).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, "PATCH", "ai/providers/nope", &root, base.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    request(
        &app,
        "POST",
        "users",
        &root,
        json!({"name":"Member","email":"m@test.local","password":"test-password-1234"}),
    )
    .await;
    let member = login(&app, "m@test.local").await;
    assert_eq!(
        request(&app, "PATCH", &route, &member, base).await.0,
        StatusCode::FORBIDDEN
    );
    drop(app);
    for suffix in ["", "-wal", "-shm", ".ai-key"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[tokio::test]
async fn conversations_are_deleted_by_their_owner_only() {
    let app = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&app, "admin@test.local").await;
    let admin = common::member(&app, &root, "owner@test.local").await;
    let (_, boot) = request(&app, "GET", "bootstrap", &admin, json!({})).await;
    let scope = json!({"kind":"workspace","id":common::team_workspace(&boot)["id"]});
    let mut ids = vec![];
    for _ in 0..3 {
        let (status, chat) =
            request(&app, "POST", "ai/chats", &admin, json!({ "scope": scope })).await;
        assert_eq!(status, StatusCode::OK, "{chat}");
        ids.push(chat["id"].as_str().unwrap().to_string());
    }
    request(
        &app,
        "POST",
        "users",
        &root,
        json!({"name":"Member","email":"m@test.local","password":"test-password-1234"}),
    )
    .await;
    let member = login(&app, "m@test.local").await;

    // Another account cannot delete it, even knowing its ID.
    let route = format!("ai/chats/{}", ids[0]);
    assert_eq!(
        request(&app, "DELETE", &route, &member, json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app, "DELETE", "ai/chats", &member, json!({}))
            .await
            .1["deleted"],
        0
    );
    assert_eq!(
        request(&app, "GET", "ai/chats", &admin, json!({}))
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        3
    );

    // The owner deletes one, then the rest.
    assert_eq!(
        request(&app, "DELETE", &route, &admin, json!({})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", &route, &admin, json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app, "DELETE", &route, &admin, json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, list) = request(&app, "GET", "ai/chats", &admin, json!({})).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let (status, cleared) = request(&app, "DELETE", "ai/chats", &admin, json!({})).await;
    assert_eq!(
        (status, cleared["deleted"].as_i64()),
        (StatusCode::OK, Some(2))
    );
    let (_, list) = request(&app, "GET", "ai/chats", &admin, json!({})).await;
    assert_eq!(list, json!([]));
}
