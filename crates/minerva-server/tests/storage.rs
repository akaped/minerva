//! Persistence: data survives a restart, databases in older layouts are migrated,
//! and a change rewrites only the rows it touches.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tower::ServiceExt;
mod common;

/// The tables with a row per entry, and the `Data` fields they hold.
const TABLES: [(&str, &str, bool); 5] = [
    ("notes", "notes", true),
    ("collaboration", "note_crdt", false),
    ("databases", "databases", true),
    ("clocks", "field_clocks", false),
    ("sessions", "sessions", false),
];

async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> Value {
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
fn open(path: &str) -> Router {
    app(Store::open(path, "admin@test.local", common::PASSWORD).unwrap())
}
/// Everything a member sees: the bootstrap, and each note with its history.
async fn snapshot(router: &Router, token: &str) -> Value {
    let boot = api(router, "GET", "bootstrap", token, json!({})).await;
    let mut notes = vec![];
    for n in boot["notes"].as_array().unwrap() {
        let id = n["id"].as_str().unwrap();
        notes.push(json!([
            api(router, "GET", &format!("notes/{id}"), token, json!({})).await,
            api(
                router,
                "GET",
                &format!("notes/{id}/history"),
                token,
                json!({})
            )
            .await,
        ]));
    }
    json!({"boot": boot, "notes": notes})
}
/// Every stored row, by table and ID.
fn rows(path: &str) -> BTreeMap<(String, String), String> {
    let db = Connection::open(path).unwrap();
    let mut all = BTreeMap::new();
    for table in ["settings"].into_iter().chain(TABLES.map(|t| t.1)) {
        let key = if table == "settings" { "key" } else { "id" };
        let mut stmt = db
            .prepare(&format!("SELECT {key}, payload FROM {table}"))
            .unwrap();
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
        {
            let (id, payload) = row.unwrap();
            all.insert((table.to_string(), id), payload);
        }
    }
    let mut stmt = db
        .prepare("SELECT note_id, seq, payload FROM note_versions")
        .unwrap();
    for row in stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap()
    {
        let (note, seq, payload) = row.unwrap();
        all.insert(("note_versions".into(), format!("{note}:{seq}")), payload);
    }
    all
}
/// Each note's earlier versions, oldest first, as layouts 0 and 1 stored them.
fn history(db: &Connection) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    let mut stmt = db
        .prepare("SELECT note_id, payload FROM note_versions ORDER BY seq")
        .unwrap();
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
    {
        let (note, payload) = row.unwrap();
        map.entry(note)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(serde_json::from_str(&payload).unwrap());
    }
    map
}
/// Rewrites a database in layout 1: each note's history in one `note_history` row.
fn downgrade_to_rows(path: &str) {
    let db = Connection::open(path).unwrap();
    let history = history(&db);
    db.execute_batch(
        "DROP TABLE note_versions; PRAGMA user_version=1;
        CREATE TABLE note_history(id TEXT PRIMARY KEY, payload TEXT NOT NULL);",
    )
    .unwrap();
    for (id, versions) in history {
        db.execute(
            "INSERT INTO note_history VALUES(?1, ?2)",
            [id, versions.to_string()],
        )
        .unwrap();
    }
}
/// Rewrites a database in the original layout: all data in one `state` row.
fn downgrade(path: &str) {
    let db = Connection::open(path).unwrap();
    let raw: String = db
        .query_row("SELECT payload FROM settings WHERE key='data'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let mut data: Value = serde_json::from_str(&raw).unwrap();
    for (field, table, list) in TABLES {
        let mut stmt = db
            .prepare(&format!("SELECT id, payload FROM {table} ORDER BY rowid"))
            .unwrap();
        let entries: Vec<(String, Value)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(|r| {
                let (id, payload) = r.unwrap();
                (id, serde_json::from_str(&payload).unwrap())
            })
            .collect();
        data[field] = if list {
            Value::Array(entries.into_iter().map(|e| e.1).collect())
        } else {
            Value::Object(entries.into_iter().collect())
        };
    }
    data["history"] = Value::Object(history(&db));
    db.execute_batch("DROP TABLE note_versions;").unwrap();
    for (_, table, _) in TABLES {
        db.execute_batch(&format!("DROP TABLE {table};")).unwrap();
    }
    db.execute_batch(
        "DROP TABLE settings; DROP TABLE note_search; PRAGMA user_version=0;
        CREATE TABLE state (id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT NOT NULL);",
    )
    .unwrap();
    db.execute("INSERT INTO state VALUES(1, ?1)", [data.to_string()])
        .unwrap();
}

#[tokio::test]
async fn data_survives_restarts_and_migration_and_edits_touch_their_rows() {
    let file = std::env::temp_dir().join(format!("minerva-storage-{}.db", uuid::Uuid::new_v4()));
    let path = file.to_str().unwrap();
    let router = open(path);
    let root = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email": "admin@test.local", "password": common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let boot = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let welcome = common::welcome(&boot)["id"].as_str().unwrap().to_string();
    // A second note with a few versions, and a database with a record.
    let other = api(
        &router,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Draft"}),
    )
    .await;
    let other = other["id"].as_str().unwrap().to_string();
    for text in ["First idea", "Second idea", "Third idea"] {
        let mut n = api(&router, "GET", &format!("notes/{other}"), &owner, json!({})).await;
        n["blocks"] = json!([{"id": "b1", "markdown": text, "x": 0, "y": 0}]);
        api(&router, "PUT", &format!("notes/{other}"), &owner, n).await;
    }
    let db = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id": folder, "name": "Reading list"}),
    )
    .await;
    let did = db["id"].as_str().unwrap();
    let name = db["columns"][0]["id"].as_str().unwrap();
    api(
        &router,
        "POST",
        &format!("databases/{did}/rows"),
        &owner,
        json!({"cells": {name: "Dune"}}),
    )
    .await;
    let before = snapshot(&router, &owner).await;
    let versions = |snapshot: &Value| {
        snapshot["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n[1].as_array().unwrap().len())
            .sum::<usize>()
    };
    assert!(versions(&before) >= 3, "earlier versions are kept");

    // A restart keeps everything, sessions included.
    drop(router);
    let router = open(path);
    assert_eq!(snapshot(&router, &owner).await, before);

    // Editing one note rewrites only that note's rows.
    let stored = rows(path);
    let mut n = api(
        &router,
        "GET",
        &format!("notes/{welcome}"),
        &owner,
        json!({}),
    )
    .await;
    n["blocks"][0]["markdown"] = json!("## A changed heading");
    api(&router, "PUT", &format!("notes/{welcome}"), &owner, n).await;
    let changed: Vec<_> = rows(path)
        .into_iter()
        .filter(|(key, payload)| stored.get(key) != Some(payload))
        .map(|(key, _)| key)
        .collect();
    assert!(!changed.is_empty());
    for (table, id) in &changed {
        assert!(
            id.contains(&welcome),
            "{table} {id} changed with another note"
        );
    }
    let before = snapshot(&router, &owner).await;
    let layout = |path: &str| -> i64 {
        Connection::open(path)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap()
    };
    assert_eq!(layout(path), 2);

    // Layout 1 (each note's history in one row) is migrated.
    drop(router);
    downgrade_to_rows(path);
    let router = open(path);
    assert_eq!(snapshot(&router, &owner).await, before);
    assert_eq!(layout(path), 2);
    let gone: bool = Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM sqlite_master WHERE name='note_history')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(gone);

    // The original single-row layout is migrated, and kept as a backup.
    drop(router);
    downgrade(path);
    let router = open(path);
    assert_eq!(snapshot(&router, &owner).await, before);
    let found = api(
        &router,
        "POST",
        "search",
        &owner,
        json!({"query": "changed heading"}),
    )
    .await;
    assert_eq!(found[0]["id"], json!(welcome), "the search index is built");
    let db = Connection::open(path).unwrap();
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let backup: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='state_v0')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(backup);
}
