//! The Reminders page: workspace reminders, one-off or repeating, with a
//! category, shared with everyone in the workspace; and the ⏰ times in notes.
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

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{uri}"))
                .header("x-minerva-client", "1")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
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

#[tokio::test]
async fn repeating_reminders_with_categories() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = call(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let other = common::member(&router, &root, "other@test.local").await;
    let viewer = common::member(&router, &root, "viewer@test.local").await;
    let editor = common::member(&router, &root, "editor@test.local").await;
    let (_, boot) = call(&router, "GET", "bootstrap", &owner, Value::Null).await;
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, users) = call(&router, "GET", "admin", &root, Value::Null).await;
    for (email, role) in [
        ("viewer@test.local", "viewer"),
        ("editor@test.local", "editor"),
    ] {
        let user_id = users["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["email"] == email)
            .unwrap()["id"]
            .clone();
        let (status, v) = call(
            &router,
            "PUT",
            "workspace-access",
            &owner,
            json!({"workspace_id":workspace,"user_id":user_id,"role":role}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{v}");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // A reminder due yesterday that repeats every year, in the "car" category.
    let (status, insurance) = call(
        &router,
        "POST",
        "reminders",
        &owner,
        json!({"workspace_id":workspace,"title":"Car insurance","category":"car",
               "at":now - 86_400,"repeat":{"every":1,"unit":"year"},"offset":60}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{insurance}");
    assert_eq!(insurance["category"], "car");
    let (status, _) = call(
        &router,
        "POST",
        "reminders",
        &owner,
        json!({"workspace_id":workspace,"title":"Oil","at":now,"repeat":{"every":0,"unit":"year"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &router,
        "POST",
        "reminders",
        &owner,
        json!({"workspace_id":workspace,"title":"","at":now}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Someone who can only view the workspace cannot add reminders to it.
    let (status, _) = call(
        &router,
        "POST",
        "reminders",
        &viewer,
        json!({"workspace_id":workspace,"title":"Mine","at":now}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Someone who cannot open the workspace cannot file reminders in it.
    let (status, _) = call(
        &router,
        "POST",
        "reminders",
        &other,
        json!({"workspace_id":workspace,"title":"Sneaky","at":now}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Done moves a repeating reminder to its next time, a year later.
    let id = insurance["id"].as_str().unwrap();
    let (status, done) = call(
        &router,
        "POST",
        &format!("reminders/{id}/done"),
        &owner,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{done}");
    let next = done["next"]["at"].as_u64().unwrap();
    assert!(
        next > now + 360 * 86_400 && next < now + 367 * 86_400,
        "{next}"
    );
    // Everyone in the workspace sees it; viewers cannot change it, editors can.
    let (_, list) = call(&router, "GET", "reminders", &viewer, Value::Null).await;
    let seen = &list["reminders"][0];
    assert_eq!(seen["title"], "Car insurance");
    assert_eq!(seen["can_edit"], false);
    assert!(!seen["created_by"].as_str().unwrap().is_empty());
    for (method, path) in [
        ("POST", format!("reminders/{id}/done")),
        ("DELETE", format!("reminders/{id}")),
    ] {
        let (status, _) = call(&router, method, &path, &viewer, Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (_, list) = call(&router, "GET", "reminders", &editor, Value::Null).await;
    assert_eq!(list["reminders"][0]["can_edit"], true);
    // Outside the workspace, nothing is seen or changed.
    let (status, _) = call(
        &router,
        "POST",
        &format!("reminders/{id}/done"),
        &other,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, list) = call(&router, "GET", "reminders", &other, Value::Null).await;
    assert!(list["reminders"].as_array().unwrap().is_empty());
    let (status, edited) = call(
        &router,
        "PUT",
        &format!("reminders/{id}"),
        &editor,
        json!({"title":"Car insurance (Allianz)","category":"car","at":next,"repeat":{"every":2,"unit":"year"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["repeat"], json!({"every":2,"unit":"year"}));

    // A one-off reminder is removed when done.
    let (_, once) = call(
        &router,
        "POST",
        "reminders",
        &owner,
        json!({"workspace_id":workspace,"title":"Blood test results","category":"health","at":now + 3600}),
    )
    .await;
    let (_, done) = call(
        &router,
        "POST",
        &format!("reminders/{}/done", once["id"].as_str().unwrap()),
        &owner,
        Value::Null,
    )
    .await;
    assert!(done["next"].is_null());

    // Times written in the workspace's notes are listed too.
    let note_id = common::welcome(&boot)["id"].as_str().unwrap().to_string();
    let (_, mut note) = call(
        &router,
        "GET",
        &format!("notes/{note_id}"),
        &owner,
        Value::Null,
    )
    .await;
    note["blocks"] = json!([{"id":"b1","markdown":"Renew the passport ⏰2027-03-01 09:00"}]);
    let (status, saved) = call(&router, "PUT", &format!("notes/{note_id}"), &owner, note).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (_, list) = call(&router, "GET", "reminders", &owner, Value::Null).await;
    assert_eq!(list["reminders"].as_array().unwrap().len(), 1);
    let from_notes = list["from_notes"].as_array().unwrap();
    assert_eq!(from_notes.len(), 1);
    assert_eq!(from_notes[0]["stamp"], "2027-03-01 09:00");
    assert_eq!(from_notes[0]["note_id"], note_id);
    // Another member does not see times in notes they cannot open.
    let (_, list) = call(&router, "GET", "reminders", &other, Value::Null).await;
    assert!(list["from_notes"].as_array().unwrap().is_empty());
}
