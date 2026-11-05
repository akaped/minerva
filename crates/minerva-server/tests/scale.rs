//! How request times grow with the number of notes (each request copies the
//! in-memory data). Not run by default: `cargo test --release -p minerva-server
//! --test scale -- --ignored --nocapture`.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use std::time::Instant;
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
#[ignore]
async fn request_times_by_number_of_notes() {
    let path = std::env::temp_dir().join(format!("minerva-scale-{}.db", uuid::Uuid::new_v4()));
    let router =
        app(Store::open(path.to_str().unwrap(), "admin@test.local", common::PASSWORD).unwrap());
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
    let text =
        "A paragraph of ordinary length, with a few words in it to index and search. ".repeat(4);
    let mut made = 0;
    println!("notes | create | open note | edit note | search | bootstrap (ms, median of 20)");
    // `SCALE=1000,5000` chooses the sizes.
    let sizes: Vec<usize> = std::env::var("SCALE")
        .unwrap_or_else(|_| "1000,3000,5000".into())
        .split(',')
        .filter_map(|n| n.trim().parse().ok())
        .collect();
    for target in sizes {
        let mut last = String::new();
        let t = Instant::now();
        while made < target {
            let (_, n) = api(&router, "POST", "notes", &owner, json!({"folder_id": folder, "title": format!("Note {made}"), "markdown": format!("{text}\n\n{text}")})).await;
            last = n["id"].as_str().unwrap().to_string();
            made += 1;
        }
        let create = t.elapsed().as_secs_f64() * 1000.0 / 1000.0_f64.max(1.0);
        let median = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let mut open = vec![];
        let mut edit = vec![];
        let mut search = vec![];
        let mut bootstrap = vec![];
        for i in 0..20 {
            let t = Instant::now();
            let (_, mut n) = api(&router, "GET", &format!("notes/{last}"), &owner, json!({})).await;
            open.push(t.elapsed().as_secs_f64() * 1000.0);
            n["title"] = json!(format!("Edited {i}"));
            let t = Instant::now();
            api(&router, "PUT", &format!("notes/{last}"), &owner, n).await;
            edit.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            api(
                &router,
                "POST",
                "search",
                &owner,
                json!({"query": "ordinary words"}),
            )
            .await;
            search.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            api(&router, "GET", "bootstrap", &owner, json!({})).await;
            bootstrap.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!(
            "{target:>6} | {create:>6.1} | {:>9.2} | {:>9.2} | {:>6.2} | {:>9.1}",
            median(open),
            median(edit),
            median(search),
            median(bootstrap)
        );
    }
    let _ = std::fs::remove_file(&path);
}
