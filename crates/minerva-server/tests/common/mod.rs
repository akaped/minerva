//! Helpers shared by the integration tests.
#![allow(dead_code)]
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

pub const PASSWORD: &str = "test-password-1234";

async fn post(router: &Router, path: &str, token: &str, body: Value) -> Value {
    send(router, "POST", path, token, body).await
}
async fn send(router: &Router, method: &str, path: &str, token: &str, body: Value) -> Value {
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
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

/// The welcome note a new server used to give its administrator, which many
/// tests read: they now find it in the member who owns their content.
const WELCOME: [&str; 4] = [
    "## A little space for big ideas",
    "Welcome to **Minerva**, your team's home for notes, knowledge, and everything in between.",
    "> Your ideas belong to you. All of your notes are stored on your own Rust server.",
    "Small thoughts. Meaningful connections. A place to begin.",
];

/// Creates a member account with `PASSWORD` (administrators only manage the
/// server and own no content) and returns its session token. Besides its
/// private "My Workspace", the account owns a shareable "Team" workspace whose
/// "General" folder holds a "Welcome to Minerva" note.
pub async fn member(router: &Router, admin: &str, email: &str) -> String {
    let token = account(router, admin, email).await;
    let boot = send(router, "GET", "bootstrap", &token, json!({})).await;
    // Tests start from the welcome note alone, as servers used to.
    for guide in boot["notes"].as_array().unwrap() {
        let path = format!("notes/{}", guide["id"].as_str().unwrap());
        let mut note = send(router, "GET", &path, &token, json!({})).await;
        note["trashed"] = json!(true);
        send(router, "PUT", &path, &token, note).await;
        send(router, "DELETE", &path, &token, json!({})).await;
    }
    // A shareable "Team" workspace, as servers used to start with.
    post(router, "workspaces", &token, json!({"name": "Team"})).await;
    let boot = send(router, "GET", "bootstrap", &token, json!({})).await;
    let folder = team_folder(&boot)["id"].clone();
    let note = post(
        router,
        "notes",
        &token,
        json!({"folder_id": folder, "title": "Welcome to Minerva"}),
    )
    .await;
    let mut full = send(
        router,
        "GET",
        &format!("notes/{}", note["id"].as_str().unwrap()),
        &token,
        json!({}),
    )
    .await;
    full["blocks"] = json!(WELCOME
        .iter()
        .enumerate()
        .map(|(i, text)| json!({"id": format!("welcome-{i}"), "markdown": text, "x": 0, "y": 0}))
        .collect::<Vec<_>>());
    send(
        router,
        "PUT",
        &format!("notes/{}", note["id"].as_str().unwrap()),
        &token,
        full,
    )
    .await;
    token
}

/// A member account without the welcome note.
pub async fn account(router: &Router, admin: &str, email: &str) -> String {
    let name = email.split('@').next().unwrap_or("Member");
    post(
        router,
        "users",
        admin,
        json!({"name": name, "email": email, "password": PASSWORD}),
    )
    .await;
    let session = post(
        router,
        "login",
        "",
        json!({"email": email, "password": PASSWORD}),
    )
    .await;
    session["token"]
        .as_str()
        .expect("member signs in")
        .to_string()
}

/// The "Team" workspace in a bootstrap, or the first workspace.
pub fn team_workspace(boot: &Value) -> &Value {
    let all = boot["workspaces"].as_array().unwrap();
    all.iter().find(|w| w["name"] == "Team").unwrap_or(&all[0])
}
/// The first folder of the "Team" workspace, or the first folder.
pub fn team_folder(boot: &Value) -> &Value {
    let team = team_workspace(boot)["id"].clone();
    let all = boot["folders"].as_array().unwrap();
    all.iter()
        .find(|f| f["workspace_id"] == team)
        .unwrap_or(&all[0])
}
/// The "Welcome to Minerva" note, or the first note.
pub fn welcome(boot: &Value) -> &Value {
    let all = boot["notes"].as_array().unwrap();
    all.iter()
        .find(|n| n["title"] == "Welcome to Minerva")
        .unwrap_or(&all[0])
}
