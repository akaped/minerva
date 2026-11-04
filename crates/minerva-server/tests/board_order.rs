//! A record's place in its board column, and number formats.
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
async fn records_keep_their_place_and_numbers_their_format() {
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
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    let columns = json!([
        {"id":"t","name":"Item","kind":"text","options":[]},
        {"id":"price","name":"Price","kind":"number","options":[],"format":"currency:EUR"},
        {"id":"share","name":"Share","kind":"number","options":[],"format":"percent"}
    ]);
    let (status, base) = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id": folder, "name": "Budget", "columns": columns}),
    )
    .await;
    assert_eq!(status, 200, "{base}");
    assert_eq!(base["columns"][1]["format"], "currency:EUR");
    let did = base["id"].as_str().unwrap().to_string();
    let (_, row) = api(
        &router,
        "POST",
        &format!("databases/{did}/rows"),
        &owner,
        json!({"cells":{"t":"Rent","price":950.5,"share":0.4}}),
    )
    .await;
    assert!(row.get("rank").is_none(), "no place until moved");

    // A place in the column, then forgotten again; nonsense is refused.
    let path = format!("databases/{did}/rows/{}", row["id"].as_str().unwrap());
    let (status, moved) = api(
        &router,
        "PATCH",
        &path,
        &owner,
        json!({"revision": row["revision"], "rank": -1.5}),
    )
    .await;
    assert_eq!(status, 200, "{moved}");
    assert_eq!(moved["rank"], -1.5);
    assert_eq!(moved["cells"]["price"], 950.5, "cells are kept");
    let (status, _) = api(
        &router,
        "PATCH",
        &path,
        &owner,
        json!({"revision": moved["revision"], "rank": "first"}),
    )
    .await;
    assert_eq!(status, 400);
    let (_, cleared) = api(
        &router,
        "PATCH",
        &path,
        &owner,
        json!({"revision": moved["revision"], "rank": null}),
    )
    .await;
    assert!(cleared.get("rank").is_none(), "{cleared}");

    // Formats only on numbers, and only known ones.
    let (_, base) = api(
        &router,
        "GET",
        &format!("databases/{did}"),
        &owner,
        json!({}),
    )
    .await;
    for (bad, column) in [
        (
            "text",
            json!({"id":"x","name":"X","kind":"text","options":[],"format":"percent"}),
        ),
        (
            "unknown",
            json!({"id":"x","name":"X","kind":"number","options":[],"format":"currency:euro"}),
        ),
    ] {
        let mut cols = base["columns"].as_array().unwrap().clone();
        cols.push(column);
        let (status, said) = api(
            &router,
            "PATCH",
            &format!("databases/{did}"),
            &owner,
            json!({"revision": base["revision"], "columns": cols}),
        )
        .await;
        assert_eq!(status, 400, "{bad}: {said}");
    }

    // Exported tables show the format.
    let (_, n) = api(
        &router,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Budget note"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    let (_, mut full) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    full["blocks"] = json!([{"id":"b0","markdown":format!("[[database:{did}]]"),"x":0,"y":0}]);
    let (status, _) = api(&router, "PUT", &format!("notes/{nid}"), &owner, full).await;
    assert_eq!(status, 200);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/export/{nid}?format=md"))
                .header("authorization", format!("Bearer {owner}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("€950.50") && text.contains("40%"), "{text}");
}
