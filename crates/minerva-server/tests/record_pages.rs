//! Records that open as pages: one page per record, named after the record,
//! kept with its database, and gone with its record or database.
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
async fn record_pages_follow_their_records() {
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
    let viewer = common::account(&router, &root, "viewer@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, viewer_boot) = api(&router, "GET", "bootstrap", &viewer, json!({})).await;
    api(
        &router,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id":folder,"user_id":viewer_boot["user"]["id"],"role":"viewer"}),
    )
    .await;
    let (_, db) = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id":folder,"name":"Projects"}),
    )
    .await;
    let did = db["id"].as_str().unwrap().to_string();
    let name = db["columns"][0]["id"].as_str().unwrap().to_string();
    let (_, row) = api(
        &router,
        "POST",
        &format!("databases/{did}/rows"),
        &owner,
        json!({"cells":{&name:"Garden shed"}}),
    )
    .await;
    let rid = row["id"].as_str().unwrap().to_string();
    let page_path = format!("databases/{did}/rows/{rid}/page");

    // Readers cannot make a page; editors make one, and opening again finds it.
    let (status, _) = api(&router, "POST", &page_path, &viewer, json!({})).await;
    assert_eq!(status, 403);
    let (status, page) = api(&router, "POST", &page_path, &owner, json!({})).await;
    assert_eq!(status, 200, "{page}");
    let pid = page["id"].as_str().unwrap().to_string();
    let (_, again) = api(&router, "POST", &page_path, &owner, json!({})).await;
    assert_eq!(again["id"], pid);
    let (_, note) = api(&router, "GET", &format!("notes/{pid}"), &viewer, json!({})).await;
    assert_eq!(note["title"], "Garden shed");
    assert_eq!(note["record"], json!({"database_id":did,"record_id":rid}));
    let (_, listed) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let summary = listed["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == pid)
        .unwrap();
    assert_eq!(summary["record"]["record_id"], rid);
    let (_, current) = api(
        &router,
        "GET",
        &format!("databases/{did}"),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(current["rows"][0]["page"], pid);

    // The page keeps its record's name: renaming the record renames it, and a
    // save with another title keeps the record's.
    api(
        &router,
        "PATCH",
        &format!("databases/{did}/rows/{rid}"),
        &owner,
        json!({"revision":row["revision"],"cells":{&name:"Garden shed v2"}}),
    )
    .await;
    let (_, mut note) = api(&router, "GET", &format!("notes/{pid}"), &owner, json!({})).await;
    assert_eq!(note["title"], "Garden shed v2");
    note["title"] = json!("Something else");
    note["blocks"] = json!([{"id":"b1","markdown":"Buy timber.","x":0,"y":0}]);
    let (status, saved) = api(&router, "PUT", &format!("notes/{pid}"), &owner, note).await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["title"], "Garden shed v2");
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("notes/{pid}"),
        &owner,
        json!({"folder_id":folder}),
    )
    .await;
    assert_eq!(status, 400, "a page stays with its database");

    // A trashed page comes back when the record is opened again.
    let (_, mut note) = api(&router, "GET", &format!("notes/{pid}"), &owner, json!({})).await;
    note["trashed"] = json!(true);
    api(&router, "PUT", &format!("notes/{pid}"), &owner, note).await;
    let (_, reopened) = api(&router, "POST", &page_path, &owner, json!({})).await;
    assert_eq!(reopened["id"], pid);
    let (_, note) = api(&router, "GET", &format!("notes/{pid}"), &owner, json!({})).await;
    assert_eq!(note["trashed"], false);

    // Deleting the record deletes its page; deleting the database, all pages.
    api(
        &router,
        "DELETE",
        &format!("databases/{did}/rows/{rid}"),
        &owner,
        json!({}),
    )
    .await;
    let (status, _) = api(&router, "GET", &format!("notes/{pid}"), &owner, json!({})).await;
    assert_eq!(status, 404);
    let (_, other) = api(
        &router,
        "POST",
        &format!("databases/{did}/rows"),
        &owner,
        json!({"cells":{&name:"Shed roof"}}),
    )
    .await;
    let (_, page) = api(
        &router,
        "POST",
        &format!(
            "databases/{did}/rows/{}/page",
            other["id"].as_str().unwrap()
        ),
        &owner,
        json!({}),
    )
    .await;
    api(
        &router,
        "DELETE",
        &format!("databases/{did}"),
        &owner,
        json!({}),
    )
    .await;
    let (status, _) = api(
        &router,
        "GET",
        &format!("notes/{}", page["id"].as_str().unwrap()),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(status, 404);
}
