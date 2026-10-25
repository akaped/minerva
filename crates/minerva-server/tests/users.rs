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

    // Delete: not yourself, not by members; the account's sign-ins, private
    // data, and unshared workspaces go.
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
        (
            status,
            deleted["workspaces_transferred"].as_u64(),
            deleted["workspaces_deleted"].as_u64()
        ),
        (StatusCode::OK, Some(0), Some(1)),
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
    // Her personal workspace was not shared: it is gone, not handed to the
    // administrator.
    assert!(!boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == workspace["id"]));
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

#[tokio::test]
async fn new_members_choose_a_password_and_get_a_default_personal_workspace() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    assert_eq!(
        boot["must_change_password"], false,
        "the first administrator chose their password"
    );

    // A member who may not create workspaces still gets a personal one.
    let (_, user) = call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Cleo","email":"cleo@test.local","password":"temporary-pass-1234"}),
    )
    .await;
    let (_, cleo) = login(&r, "cleo@test.local", "temporary-pass-1234").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &cleo, json!({})).await;
    assert_eq!(
        (
            boot["must_change_password"].as_bool(),
            boot["owns_workspace"].as_bool()
        ),
        (Some(true), Some(false))
    );
    assert_eq!(boot["default_workspace"], Value::Null);
    assert_eq!(
        call(&r, "POST", "workspaces", &cleo, json!({"name":"Mine"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    let (status, done) = call(&r, "POST", "account/password", &cleo, json!({"current_password":"temporary-pass-1234","new_password":"cleos-own-password","workspace_name":"Personal"})).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let personal = done["workspace_id"].as_str().unwrap().to_string();
    let (_, boot) = call(&r, "GET", "bootstrap", &cleo, json!({})).await;
    assert_eq!(boot["must_change_password"], false);
    assert_eq!(boot["default_workspace"], personal.as_str());
    let w = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == personal.as_str())
        .unwrap();
    assert_eq!(
        (w["name"].as_str(), w["owner_id"].as_str()),
        (Some("Personal"), user["id"].as_str())
    );
    assert!(
        boot["folders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["workspace_id"] == personal.as_str()),
        "with a first folder"
    );

    // Changing the password again creates nothing more.
    let (_, again) = call(&r, "POST", "account/password", &cleo, json!({"current_password":"cleos-own-password","new_password":"cleos-next-password","workspace_name":"Second"})).await;
    assert_eq!(again["workspace_id"], Value::Null);

    // A password set by an administrator must be replaced again.
    call(
        &r,
        "PATCH",
        &format!("users/{}", user["id"].as_str().unwrap()),
        &admin,
        json!({"password":"reset-by-admin-123"}),
    )
    .await;
    let (_, cleo) = login(&r, "cleo@test.local", "reset-by-admin-123").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &cleo, json!({})).await;
    assert_eq!(
        (
            boot["must_change_password"].as_bool(),
            boot["owns_workspace"].as_bool()
        ),
        (Some(true), Some(true))
    );
}

#[tokio::test]
async fn shared_workspaces_of_a_deleted_account_pass_to_the_administrator() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    let (_, eve) = call(&r, "POST", "users", &admin, json!({"name":"Eve","email":"eve@test.local","password":"eve-password-1234","can_create_workspace":true})).await;
    let (_, finn) = call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Finn","email":"finn@test.local","password":"finn-password-123"}),
    )
    .await;
    let (_, eve_token) = login(&r, "eve@test.local", "eve-password-1234").await;
    let (_, team) = call(&r, "POST", "workspaces", &eve_token, json!({"name":"Team"})).await;
    let (_, private) = call(
        &r,
        "POST",
        "workspaces",
        &eve_token,
        json!({"name":"Diary"}),
    )
    .await;
    // The administrator cannot see either of them.
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    assert!(!boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == team["id"] || w["id"] == private["id"]));
    let (status, _) = call(
        &r,
        "PUT",
        "workspace-access",
        &eve_token,
        json!({"workspace_id":team["id"],"user_id":finn["id"],"role":"editor"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, deleted) = call(
        &r,
        "DELETE",
        &format!("users/{}", eve["id"].as_str().unwrap()),
        &admin,
        json!({}),
    )
    .await;
    assert_eq!(
        (
            deleted["workspaces_transferred"].as_u64(),
            deleted["workspaces_deleted"].as_u64()
        ),
        (Some(1), Some(1))
    );
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    let ids: Vec<_> = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].clone())
        .collect();
    assert!(ids.contains(&team["id"]) && !ids.contains(&private["id"]));
    let (_, finn_token) = login(&r, "finn@test.local", "finn-password-123").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &finn_token, json!({})).await;
    assert!(
        boot["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"] == team["id"]),
        "its members keep it"
    );
}
