//! Editing comments, comments in the Mentions list, and guests with an edit
//! link commenting without learning who the members are.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn call(
    r: &Router,
    method: &str,
    path: &str,
    auth: (&str, &str),
    body: Value,
) -> (u16, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("content-type", "application/json")
                .header("x-minerva-client", "1")
                .header(auth.0, auth.1)
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
async fn api(r: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    call(
        r,
        method,
        path,
        ("authorization", &format!("Bearer {token}")),
        body,
    )
    .await
}
async fn guest(r: &Router, method: &str, path: &str, link: &str, body: Value) -> (u16, Value) {
    call(
        r,
        method,
        &format!("shared/{path}"),
        ("cookie", &format!("minerva_share={link}")),
        body,
    )
    .await
}

#[tokio::test]
async fn comments_are_edited_mentioned_and_open_to_editing_guests() {
    let r = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&r, &root, "owner@test.local").await;
    let (_, boot) = api(&r, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let friend = common::account(&r, &root, "friend@test.local").await;
    let (_, them) = api(&r, "GET", "bootstrap", &friend, json!({})).await;
    api(
        &r,
        "PUT",
        "workspace-access",
        &owner,
        json!({"workspace_id": workspace, "user_id": them["user"]["id"], "role": "editor"}),
    )
    .await;
    let (_, n) = api(
        &r,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Plan"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    let comments = format!("notes/{nid}/comments");

    // An author edits their comment; no one else can.
    let (_, thread) = api(
        &r,
        "POST",
        &comments,
        &owner,
        json!({"text": "First thought"}),
    )
    .await;
    let tid = thread["id"].as_str().unwrap().to_string();
    let cid = thread["comments"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(thread["comments"][0]["mine"], true);
    let (status, edited) = api(
        &r,
        "PATCH",
        &format!("{comments}/{tid}/{cid}"),
        &owner,
        json!({"text": "Second thought, @friend@test.local"}),
    )
    .await;
    assert_eq!(status, 200, "{edited}");
    assert_eq!(
        edited["comments"][0]["text"],
        "Second thought, @friend@test.local"
    );
    assert!(edited["comments"][0]["edited_at"].is_u64());
    let (status, _) = api(
        &r,
        "PATCH",
        &format!("{comments}/{tid}/{cid}"),
        &friend,
        json!({"text": "Mine now"}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = api(
        &r,
        "PATCH",
        &format!("{comments}/{tid}/{cid}"),
        &owner,
        json!({"text": "  "}),
    )
    .await;
    assert_eq!(status, 400);

    // The mention shows in the friend's Mentions, from the comment.
    let (_, mentions) = api(&r, "GET", "mentions", &friend, json!({})).await;
    let found = mentions
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["comment"] == true)
        .expect("the comment is listed");
    assert_eq!(found["id"], nid.as_str());
    assert_eq!(found["thread"], tid.as_str());
    let (_, own) = api(&r, "GET", "mentions", &owner, json!({})).await;
    assert!(
        own.as_array().unwrap().iter().all(|m| m["comment"] != true),
        "not one's own"
    );

    // A guest with an edit link comments; with a view link, cannot.
    let (_, edit_link) = api(
        &r,
        "POST",
        &format!("notes/{nid}/shares"),
        &owner,
        json!({"kind":"link","role":"editor"}),
    )
    .await;
    let (_, view_link) = api(
        &r,
        "POST",
        &format!("notes/{nid}/shares"),
        &owner,
        json!({"kind":"link","role":"viewer"}),
    )
    .await;
    let edit_link = edit_link["token"].as_str().unwrap().to_string();
    let view_link = view_link["token"].as_str().unwrap().to_string();
    let (status, _) = guest(&r, "GET", "comments", &view_link, json!({})).await;
    assert_eq!(status, 403);
    let (status, said) = guest(
        &r,
        "POST",
        "comments",
        &edit_link,
        json!({"text": "A guest's idea"}),
    )
    .await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(said["comments"][0]["author_name"], "Guest");
    assert_eq!(said["comments"][0]["mine"], true);
    let (status, listed) = guest(&r, "GET", "comments", &edit_link, json!({})).await;
    assert_eq!(status, 200);
    let member_thread = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == tid.as_str())
        .unwrap();
    assert_eq!(
        member_thread["comments"][0]["author_name"], "Member",
        "members stay unnamed"
    );
    assert_eq!(member_thread["comments"][0]["author"], "someone");
    assert_eq!(member_thread["comments"][0]["mine"], false);
    // The guest replies to the member, and cannot change the member's words.
    let (status, _) = guest(
        &r,
        "POST",
        &format!("comments/{tid}/replies"),
        &edit_link,
        json!({"text": "Agreed"}),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = guest(
        &r,
        "PATCH",
        &format!("comments/{tid}/{cid}"),
        &edit_link,
        json!({"text": "Changed"}),
    )
    .await;
    assert_eq!(status, 403);
    // Members see the guest as "Guest".
    let (_, seen) = api(&r, "GET", &comments, &owner, json!({})).await;
    let replies = &seen
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == tid.as_str())
        .unwrap()["comments"];
    assert_eq!(replies[1]["author_name"], "Guest");
}
