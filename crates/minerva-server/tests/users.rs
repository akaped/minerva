//! Account management: administrators edit and delete accounts; everyone can
//! change their own password.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn call(r: &Router, method: &str, path: &str, token: &str, v: Value) -> (StatusCode, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
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
async fn login(r: &Router, email: &str, password: &str) -> (StatusCode, String) {
    let (status, v) = call(
        r,
        "POST",
        "login",
        "",
        json!({"email":email,"password":password}),
    )
    .await;
    (status, v["token"].as_str().unwrap_or("").into())
}

#[tokio::test]
async fn administrators_edit_and_delete_accounts_and_people_change_passwords() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    let (_, user) = call(&r, "POST", "users", &admin, json!({"name":"Ada","email":"ada@test.local","password":"ada-password-1234","can_create_workspace":true})).await;
    let uid = user["id"].as_str().unwrap().to_string();
    call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Bob","email":"bob@test.local","password":"bob-password-1234"}),
    )
    .await;

    // Edit: name, email, administrator; duplicates and self-edits are refused.
    let (status, edited) = call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"name":"Ada Lovelace","email":"Ada.L@Test.local"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(
        (edited["name"].as_str(), edited["email"].as_str()),
        (Some("Ada Lovelace"), Some("ada.l@test.local"))
    );
    let (status, _) = call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"email":"bob@test.local"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, me) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    let admin_id = me["user"]["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        &r,
        "PATCH",
        &format!("users/{admin_id}"),
        &admin,
        json!({"name":"Me"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, promoted) = call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"admin":true}),
    )
    .await;
    assert_eq!(
        (status, promoted["admin"].as_bool()),
        (StatusCode::OK, Some(true))
    );
    call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"admin":false}),
    )
    .await;

    // Changing your own password needs the current one, ends other sessions,
    // and keeps this one.
    let (_, ada) = login(&r, "ada.l@test.local", "ada-password-1234").await;
    let (_, ada_phone) = login(&r, "ada.l@test.local", "ada-password-1234").await;
    let (status, _) = call(
        &r,
        "POST",
        "account/password",
        &ada,
        json!({"current_password":"wrong-password-1234","new_password":"new-ada-password-1"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &r,
        "POST",
        "account/password",
        &ada,
        json!({"current_password":"ada-password-1234","new_password":"short"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &r,
        "POST",
        "account/password",
        &ada,
        json!({"current_password":"ada-password-1234","new_password":"new-ada-password-1"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&r, "GET", "bootstrap", &ada, json!({})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&r, "GET", "bootstrap", &ada_phone, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&r, "ada.l@test.local", "ada-password-1234").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&r, "ada.l@test.local", "new-ada-password-1").await.0,
        StatusCode::OK
    );

    // Ada owns a workspace and has a private deck and chat.
    let (_, workspace) = call(
        &r,
        "POST",
        "workspaces",
        &ada,
        json!({"name":"Ada's research"}),
    )
    .await;
    let (status, _) = call(
        &r,
        "POST",
        "flashcards/decks",
        &ada,
        json!({"name":"Private deck"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        &r,
        "POST",
        "ai/chats",
        &ada,
        json!({"scope":{"kind":"workspace","id":workspace["id"]}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Delete: not yourself, not by members; the workspace passes to the
    // administrator, and the account's sign-ins and private data go.
    let (status, _) = call(
        &r,
        "DELETE",
        &format!("users/{admin_id}"),
        &admin,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&r, "DELETE", &format!("users/{admin_id}"), &ada, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, deleted) = call(&r, "DELETE", &format!("users/{uid}"), &admin, json!({})).await;
    assert_eq!(
        (status, deleted["workspaces_transferred"].as_u64()),
        (StatusCode::OK, Some(1)),
        "{deleted}"
    );
    assert_eq!(
        call(&r, "GET", "bootstrap", &ada, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&r, "ada.l@test.local", "new-ada-password-1").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    let kept = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == workspace["id"])
        .unwrap();
    assert_eq!(kept["owner_id"], admin_id.as_str());
    let (_, list) = call(&r, "GET", "admin", &admin, json!({})).await;
    assert!(!list["users"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["id"] == uid.as_str()));
    // The same address can be used again, with nothing of the old account.
    let (_, again) = call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Ada","email":"ada.l@test.local","password":"ada-password-1234"}),
    )
    .await;
    assert!(again["id"].is_string());
    let (_, ada) = login(&r, "ada.l@test.local", "ada-password-1234").await;
    let (_, decks) = call(&r, "GET", "flashcards/decks", &ada, json!({})).await;
    assert_eq!(decks["decks"], json!([]));
    let (_, chats) = call(&r, "GET", "ai/chats", &ada, json!({})).await;
    assert_eq!(chats, json!([]));
}
