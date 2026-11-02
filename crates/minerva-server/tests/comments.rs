//! Comments: threads on a block or a note, replies, resolving, who may do what,
//! and comments going with their note.
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
async fn threads_replies_resolving_and_permissions() {
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
    let stranger = common::account(&router, &root, "stranger@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let note = common::welcome(&boot)["id"].as_str().unwrap().to_string();
    let (_, reader_boot) = api(&router, "GET", "bootstrap", &reader, json!({})).await;
    let reader_id = reader_boot["user"]["id"].clone();
    api(
        &router,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id":folder,"user_id":reader_id,"role":"viewer"}),
    )
    .await;
    let path = format!("notes/{note}/comments");

    // Readers comment too; strangers cannot see or add comments.
    let (status, thread) = api(
        &router,
        "POST",
        &path,
        &reader,
        json!({"block_id":"welcome-1","quote":"your team's home","text":"Whose team?"}),
    )
    .await;
    assert_eq!(status, 200, "{thread}");
    assert_eq!(thread["comments"][0]["author_name"], "reader");
    assert_eq!(thread["quote"], "your team's home");
    let tid = thread["id"].as_str().unwrap().to_string();
    let (status, _) = api(&router, "GET", &path, &stranger, json!({})).await;
    assert_eq!(status, 403);
    let (status, _) = api(&router, "POST", &path, &stranger, json!({"text":"Hi"})).await;
    assert_eq!(status, 403);
    for bad in [json!({"text":"  "}), json!({"block_id":"gone","text":"Hi"})] {
        let (status, _) = api(&router, "POST", &path, &owner, bad.clone()).await;
        assert_eq!(status, 400, "{bad}");
    }
    let (status, page) = api(
        &router,
        "POST",
        &path,
        &owner,
        json!({"text":"A comment on the whole note"}),
    )
    .await;
    assert_eq!(status, 200);
    assert!(page["block_id"].is_null());

    // Replies, and resolving: by an editor or the thread's author.
    let (_, replied) = api(
        &router,
        "POST",
        &format!("{path}/{tid}/replies"),
        &owner,
        json!({"text":"Ours."}),
    )
    .await;
    assert_eq!(replied["comments"].as_array().unwrap().len(), 2);
    let page_id = page["id"].as_str().unwrap();
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("{path}/{page_id}"),
        &reader,
        json!({"resolved":true}),
    )
    .await;
    assert_eq!(status, 403, "a reader cannot resolve someone else's thread");
    let (status, resolved) = api(
        &router,
        "PATCH",
        &format!("{path}/{tid}"),
        &reader,
        json!({"resolved":true}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        (
            resolved["resolved"].clone(),
            resolved["resolved_by_name"].clone()
        ),
        (json!(true), json!("reader"))
    );
    let (_, reopened) = api(
        &router,
        "POST",
        &format!("{path}/{tid}/replies"),
        &owner,
        json!({"text":"One more thing"}),
    )
    .await;
    assert_eq!(
        reopened["resolved"], false,
        "a reply opens a resolved thread again"
    );

    // Only authors delete their comments; the first one takes the thread with it.
    let reply = reopened["comments"][1]["id"].as_str().unwrap();
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("{path}/{tid}/{reply}"),
        &reader,
        json!({}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, after) = api(
        &router,
        "DELETE",
        &format!("{path}/{tid}/{reply}"),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, after["comments"].as_array().unwrap().len()),
        (200, 2)
    );
    let first = thread["comments"][0]["id"].as_str().unwrap();
    api(
        &router,
        "DELETE",
        &format!("{path}/{tid}/{first}"),
        &reader,
        json!({}),
    )
    .await;
    let (_, all) = api(&router, "GET", &path, &owner, json!({})).await;
    assert_eq!(
        all.as_array().unwrap().len(),
        1,
        "only the note's own thread is left"
    );

    // Comments go with their note.
    let (_, mut n) = api(&router, "GET", &format!("notes/{note}"), &owner, json!({})).await;
    n["trashed"] = json!(true);
    api(&router, "PUT", &format!("notes/{note}"), &owner, n).await;
    api(
        &router,
        "DELETE",
        &format!("notes/{note}"),
        &owner,
        json!({}),
    )
    .await;
    let (status, _) = api(&router, "GET", &path, &owner, json!({})).await;
    assert_eq!(status, 404);
}
