//! Search beyond notes: database records and the text of attached files, and
//! the folder, tag and date filters.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn send(r: &Router, method: &str, path: &str, token: &str, body: Vec<u8>) -> (u16, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("x-minerva-client", "1")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body))
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
    send(r, method, path, token, body.to_string().into_bytes()).await
}
async fn note(r: &Router, token: &str, folder: &str, title: &str, text: &str) -> String {
    let (_, created) = api(
        r,
        "POST",
        "notes",
        token,
        json!({"folder_id": folder, "title": title}),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let (_, mut n) = api(r, "GET", &format!("notes/{id}"), token, json!({})).await;
    n["blocks"] = json!([{"id": format!("{id}-0"), "markdown": text, "x": 0, "y": 0}]);
    let (status, _) = api(r, "PUT", &format!("notes/{id}"), token, n).await;
    assert_eq!(status, 200);
    id
}
async fn search(r: &Router, token: &str, body: Value) -> Vec<Value> {
    let (status, found) = api(r, "POST", "search", token, body).await;
    assert_eq!(status, 200, "{found}");
    found.as_array().unwrap().clone()
}
fn of_kind<'a>(found: &'a [Value], kind: &str) -> Vec<&'a str> {
    found
        .iter()
        .filter(|f| f["kind"] == kind)
        .map(|f| f["title"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn records_files_and_filters() {
    let r = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let admin = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&r, &admin, "owner@test.local").await;
    let stranger = common::member(&r, &admin, "stranger@test.local").await;
    let (_, boot) = api(&r, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, sub) = api(
        &r,
        "POST",
        "folders",
        &owner,
        json!({"workspace_id": workspace, "name": "Trips", "parent_id": folder}),
    )
    .await;
    let sub = sub["id"].as_str().unwrap().to_string();

    // A database's records are found by their title and their other properties.
    let (_, base) = api(
        &r,
        "POST",
        "databases",
        &owner,
        json!({"folder_id": folder, "name": "Reading list"}),
    )
    .await;
    let did = base["id"].as_str().unwrap().to_string();
    let title = base["columns"][0]["id"].as_str().unwrap().to_string();
    let (status, _) = api(
        &r,
        "POST",
        &format!("databases/{did}/rows"),
        &owner,
        json!({"cells": {title.clone(): "Perché le stelle brillano"}}),
    )
    .await;
    assert_eq!(status, 200);
    let found = search(
        &r,
        &owner,
        json!({"query": "perche stell", "include": ["records"]}),
    )
    .await;
    assert_eq!(of_kind(&found, "record"), vec!["Perché le stelle brillano"]);
    assert_eq!(found[0]["database_name"], "Reading list");
    // Not without asking, and not for someone who cannot open the database.
    let found = search(&r, &owner, json!({"query": "stelle"})).await;
    assert!(of_kind(&found, "record").is_empty());
    let found = search(
        &r,
        &stranger,
        json!({"query": "stelle", "include": ["records"]}),
    )
    .await;
    assert!(found.is_empty());
    // A renamed record is found by its new name only.
    let (_, base) = api(&r, "GET", &format!("databases/{did}"), &owner, json!({})).await;
    let row = &base["rows"][0];
    let (status, _) = api(
        &r,
        "PATCH",
        &format!("databases/{did}/rows/{}", row["id"].as_str().unwrap()),
        &owner,
        json!({"revision": row["revision"], "cells": {title.clone(): "Galaxies"}}),
    )
    .await;
    assert_eq!(status, 200);
    let found = search(
        &r,
        &owner,
        json!({"query": "stelle", "include": ["records"]}),
    )
    .await;
    assert!(of_kind(&found, "record").is_empty());
    let found = search(
        &r,
        &owner,
        json!({"query": "galax", "include": ["records"]}),
    )
    .await;
    assert_eq!(of_kind(&found, "record"), vec!["Galaxies"]);

    // A text file's words, once the worker has read them.
    let holder = note(&r, &owner, &folder, "Receipts", "Files below").await;
    let (status, file) = send(
        &r,
        "POST",
        &format!("notes/{holder}/attachments?filename=hotel.txt"),
        &owner,
        b"Booking confirmation for the Albergo Miramare in Sorrento".to_vec(),
    )
    .await;
    assert_eq!(status, 201, "{file}");
    let mut files = vec![];
    for _ in 0..100 {
        let found = search(
            &r,
            &owner,
            json!({"query": "miramare", "include": ["files"]}),
        )
        .await;
        files = found.into_iter().filter(|f| f["kind"] == "file").collect();
        if !files.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(files.len(), 1, "the file's text is indexed");
    assert_eq!(files[0]["note_id"], holder.as_str());
    assert_eq!(files[0]["title"], "hotel.txt");
    assert!(files[0]["snippet"]
        .as_str()
        .unwrap()
        .contains("\u{1}Miramare\u{2}"));
    let found = search(
        &r,
        &stranger,
        json!({"query": "miramare", "include": ["files"]}),
    )
    .await;
    assert!(found.is_empty());

    // Filters: a folder with its subfolders, a tag, a recent change.
    let trip = note(
        &r,
        &owner,
        &sub,
        "Sorrento trip",
        "Lemons everywhere #travel",
    )
    .await;
    let other = note(&r, &owner, &folder, "Lemon tart", "A recipe with lemons").await;
    let ids = |found: &[Value]| -> Vec<String> {
        found
            .iter()
            .map(|f| f["id"].as_str().unwrap().to_string())
            .collect()
    };
    let found = search(&r, &owner, json!({"query": "lemons", "folder_id": sub})).await;
    assert_eq!(ids(&found), vec![trip.clone()]);
    let found = search(&r, &owner, json!({"query": "lemons", "folder_id": folder})).await;
    assert_eq!(found.len(), 2, "the parent folder takes in its subfolders");
    let found = search(&r, &owner, json!({"query": "lemons", "tag": "#travel"})).await;
    assert_eq!(ids(&found), vec![trip.clone()]);
    let found = search(
        &r,
        &owner,
        json!({"query": "lemons", "updated_after": 4_000_000_000u64}),
    )
    .await;
    assert!(found.is_empty());
    // Filters alone list what they keep, latest first.
    let found = search(&r, &owner, json!({"query": "", "tag": "travel"})).await;
    assert_eq!(ids(&found), vec![trip]);
    assert!(!ids(&search(&r, &owner, json!({"query": "", "folder_id": folder})).await).is_empty());
    let _ = other;
}
