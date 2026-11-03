//! The server's own words in the reader's language: a new account's guide,
//! published pages for each visitor, and exports for the person exporting.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn send(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    language: &str,
    body: Value,
) -> (u16, String) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .header("x-minerva-client", "1")
                .header("accept-language", language)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> Value {
    let (_, text) = send(
        router,
        method,
        &format!("/api/v1/{path}"),
        token,
        "en",
        body,
    )
    .await;
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

#[tokio::test]
async fn guides_pages_and_exports_speak_the_readers_language() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let root = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();

    // An account made from an Italian browser starts with the Italian guide.
    let (status, _) = send(
        &router,
        "POST",
        "/api/v1/users",
        &root,
        "it-IT,it;q=0.9,en;q=0.5",
        json!({"name":"Lia","email":"lia@test.local","password":common::PASSWORD}),
    )
    .await;
    assert_eq!(status, 200);
    let lia = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"lia@test.local","password":common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let boot = api(&router, "GET", "bootstrap", &lia, json!({})).await;
    let guide = &boot["notes"][0];
    assert_eq!(guide["title"], "Per iniziare");
    assert_eq!(boot["folders"][0]["name"], "Generale");
    let workspace = &boot["workspaces"][0];
    assert_eq!(
        workspace["name"], "My Workspace",
        "shown translated by the app"
    );
    let full = api(
        &router,
        "GET",
        &format!("notes/{}", guide["id"].as_str().unwrap()),
        &lia,
        json!({}),
    )
    .await;
    assert!(full["blocks"][0]["markdown"]
        .as_str()
        .unwrap()
        .starts_with("Benvenuto!"));

    // Lia reads the app in Italian: her export's own words are Italian.
    api(
        &router,
        "PUT",
        "account/language",
        &lia,
        json!({"language":"it"}),
    )
    .await;
    let folder = boot["folders"][0]["id"].clone();
    let note = api(
        &router,
        "POST",
        "notes",
        &lia,
        json!({"folder_id":folder,"title":"Diario di bordo","markdown":"[[toc]]\n\n## Primo\n\n![Persa](#note/0b8f5f9e-0000-4000-8000-000000000000)"}),
    )
    .await;
    let nid = note["id"].as_str().unwrap().to_string();
    let (_, md) = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=md&front_matter=0"),
        &lia,
        "en",
        Value::Null,
    )
    .await;
    assert!(md.contains("*Nota incorporata (eliminata)*"), "{md}");
    let (_, html) = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=html"),
        &lia,
        "en",
        Value::Null,
    )
    .await;
    assert!(html.contains(">Indice<"), "{html}");

    // Published pages follow each visitor's browser.
    api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        &lia,
        json!({"slug":"diario","mode":"live"}),
    )
    .await;
    let (_, italian) = send(&router, "GET", "/shared/", "", "it", Value::Null).await;
    assert!(
        italian.contains("<html lang=\"it\"") && italian.contains("Note pubblicate"),
        "{italian}"
    );
    let (_, english) = send(&router, "GET", "/shared/", "", "en-GB", Value::Null).await;
    assert!(english.contains("<html lang=\"en\"") && english.contains("Published notes"));
    let (status, missing) = send(&router, "GET", "/shared/nope", "", "it", Value::Null).await;
    assert_eq!(status, 404);
    assert!(missing.contains("Pagina non trovata"), "{missing}");
    let (_, page) = send(&router, "GET", "/shared/diario", "", "it", Value::Null).await;
    assert!(
        page.contains("Nota incorporata · visibile ai membri"),
        "{page}"
    );
}
