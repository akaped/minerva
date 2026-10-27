//! Permanent deletion of notes, folders, and workspaces, and who may do it.
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

const PASSWORD: &str = "test-password-1234";

async fn send(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Body,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(body)
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(
        router,
        method,
        &format!("/api/v1/{path}"),
        token,
        Body::from(body.to_string()),
    )
    .await
}
async fn login(router: &Router, email: &str) -> String {
    let (_, v) = api(
        router,
        "POST",
        "login",
        "",
        json!({"email":email,"password":PASSWORD}),
    )
    .await;
    v["token"].as_str().unwrap().to_string()
}
async fn user(router: &Router, admin: &str, name: &str) -> (String, String) {
    let email = format!("{name}@test.local");
    let (_, v) = api(
        router,
        "POST",
        "users",
        admin,
        json!({"name":name,"email":email,"password":PASSWORD,"can_create_workspace":true}),
    )
    .await;
    (
        v["id"].as_str().unwrap().to_string(),
        login(router, &email).await,
    )
}
async fn note(router: &Router, token: &str, folder: &str, title: &str) -> String {
    let (_, n) = api(
        router,
        "POST",
        "notes",
        token,
        json!({"folder_id":folder,"title":title}),
    )
    .await;
    n["id"].as_str().unwrap().to_string()
}
async fn trash(router: &Router, token: &str, nid: &str) {
    let (_, mut n) = api(router, "GET", &format!("notes/{nid}"), token, json!({})).await;
    n["trashed"] = json!(true);
    let (status, _) = api(router, "PUT", &format!("notes/{nid}"), token, n).await;
    assert_eq!(status, StatusCode::OK);
}
fn workspace<'a>(boot: &'a Value, wid: &str) -> Option<&'a Value> {
    boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == wid)
}

#[tokio::test]
async fn editors_delete_notes_and_folders_and_owners_delete_personal_workspaces() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let (_, alice) = user(&router, &root, "alice").await;
    let (bob_id, bob) = user(&router, &root, "bob").await;
    let (_, w) = api(
        &router,
        "POST",
        "workspaces",
        &alice,
        json!({"name":"Alpha"}),
    )
    .await;
    let wid = w["id"].as_str().unwrap().to_string();
    let (_, boot) = api(&router, "GET", "bootstrap", &alice, json!({})).await;
    assert_eq!(
        workspace(&boot, &wid).unwrap()["can_delete"],
        true,
        "personal workspaces can be deleted by their owner"
    );
    let general = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] == wid)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, other) = api(
        &router,
        "POST",
        "folders",
        &alice,
        json!({"workspace_id":wid,"name":"Other"}),
    )
    .await;
    let other = other["id"].as_str().unwrap().to_string();

    // A note with a file, a share, and a published page.
    let nid = note(&router, &alice, &general, "Doomed").await;
    let (_, file) = send(
        &router,
        "POST",
        &format!("/api/v1/notes/{nid}/attachments?filename=a.txt"),
        &alice,
        Body::from("bytes"),
    )
    .await;
    let file = file["id"].as_str().unwrap().to_string();
    api(
        &router,
        "POST",
        &format!("notes/{nid}/shares"),
        &alice,
        json!({"kind":"member","email":"bob@test.local","role":"editor"}),
    )
    .await;
    api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        &alice,
        json!({"slug":"doomed","mode":"live"}),
    )
    .await;
    let (status, _) = send(&router, "GET", "/shared/doomed", "", Body::empty()).await;
    assert_eq!(status, StatusCode::OK);

    // Notes: only folder editors, and only from the trash.
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("notes/{nid}"),
        &alice,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "notes go to the trash first"
    );
    trash(&router, &alice, &nid).await;
    let (status, _) = api(&router, "DELETE", &format!("notes/{nid}"), &bob, json!({})).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "share recipients cannot delete notes"
    );
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("notes/{nid}"),
        &alice,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        api(&router, "GET", &format!("notes/{nid}"), &alice, json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/api/v1/attachments/{file}"),
            &alice,
            Body::empty()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&router, "GET", "/shared/doomed", "", Body::empty())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, sharing) = api(&router, "GET", "sharing", &bob, json!({})).await;
    assert_eq!(sharing["shared_with_me"], json!([]));

    // Shared or not, only the owner deletes a workspace: not its members, not
    // an administrator.
    api(
        &router,
        "PUT",
        "workspace-access",
        &alice,
        json!({"workspace_id":wid,"user_id":bob_id,"role":"viewer"}),
    )
    .await;
    let (_, boot) = api(&router, "GET", "bootstrap", &alice, json!({})).await;
    assert_eq!(workspace(&boot, &wid).unwrap()["can_delete"], true);
    for (who, token) in [("member", &bob), ("administrator", &admin)] {
        let (status, _) = api(
            &router,
            "DELETE",
            &format!("workspaces/{wid}"),
            token,
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}");
    }

    // Folders: viewers cannot delete them, editors can, with their contents.
    let inside = note(&router, &alice, &other, "Inside").await;
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("folders/{other}"),
        &bob,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    api(
        &router,
        "PUT",
        "workspace-access",
        &alice,
        json!({"workspace_id":wid,"user_id":bob_id,"role":"editor"}),
    )
    .await;
    let (status, done) = api(
        &router,
        "DELETE",
        &format!("folders/{other}"),
        &bob,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, done["deleted_notes"].as_u64()),
        (StatusCode::OK, Some(1))
    );
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{inside}"),
            &alice,
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("workspaces/{wid}"),
        &bob,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "members never delete workspaces"
    );

    // Once personal again, the owner deletes it and everything in it.
    let (_, site) = api(
        &router,
        "POST",
        "sites",
        &alice,
        json!({"workspace_id":wid,"path":"alpha-blog"}),
    )
    .await;
    assert!(site["id"].is_string());
    let (_, db) = api(
        &router,
        "POST",
        "databases",
        &alice,
        json!({"folder_id":general,"name":"Tasks"}),
    )
    .await;
    api(
        &router,
        "POST",
        "mcp-credentials",
        &alice,
        json!({"workspace_id":wid,"name":"Tool","expires_days":1}),
    )
    .await;
    api(
        &router,
        "PUT",
        "workspace-access",
        &alice,
        json!({"workspace_id":wid,"user_id":bob_id,"role":"none"}),
    )
    .await;
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("workspaces/{wid}"),
        &alice,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, boot) = api(&router, "GET", "bootstrap", &alice, json!({})).await;
    assert!(workspace(&boot, &wid).is_none());
    assert!(!boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == general.as_str()));
    assert_eq!(boot["sites"], json!([]));
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("databases/{}", db["id"].as_str().unwrap()),
            &admin,
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, creds) = api(&router, "GET", "mcp-credentials", &alice, json!({})).await;
    assert_eq!(creds, json!([]));
    assert_eq!(
        send(&router, "GET", "/alpha-blog/", "", Body::empty())
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // Administrators cannot see or delete other people's workspaces.
    let (_, w) = api(&router, "POST", "workspaces", &bob, json!({"name":"Beta"})).await;
    let beta = w["id"].as_str().unwrap();
    let (_, boot) = api(&router, "GET", "bootstrap", &admin, json!({})).await;
    assert!(workspace(&boot, beta).is_none());
    assert_eq!(
        api(
            &router,
            "DELETE",
            &format!("workspaces/{beta}"),
            &admin,
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
