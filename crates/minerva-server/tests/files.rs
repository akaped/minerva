//! Files attached to notes: PDFs open in the page, in a tab beside the note;
//! other files only download.
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

async fn send(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("x-minerva-client", "1")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, bytes)
}

#[tokio::test]
async fn pdfs_open_in_the_page_and_other_files_download() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, _, login) = send(
        &router,
        "POST",
        "/api/v1/login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD})
            .to_string()
            .into_bytes(),
    )
    .await;
    let root = serde_json::from_slice::<Value>(&login).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let (_, _, boot) = send(&router, "GET", "/api/v1/bootstrap", &owner, vec![]).await;
    let boot: Value = serde_json::from_slice(&boot).unwrap();
    let note = common::welcome(&boot)["id"].as_str().unwrap().to_string();

    let upload = |name: &'static str, bytes: Vec<u8>| {
        let router = router.clone();
        let owner = owner.clone();
        let note = note.clone();
        async move {
            let (status, _, body) = send(
                &router,
                "POST",
                &format!("/api/v1/notes/{note}/attachments?filename={name}"),
                &owner,
                bytes,
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
            serde_json::from_slice::<Value>(&body).unwrap()
        }
    };
    let pdf = upload("report.pdf", b"%PDF-1.4\n%test\n".to_vec()).await;
    assert_eq!(pdf["media_type"], "application/pdf");
    let zip = upload("data.zip", b"PK\x03\x04test".to_vec()).await;
    assert_eq!(zip["media_type"], "application/octet-stream");

    // The PDF opens in the page, and Minerva's own pages may show it in a frame.
    let (status, headers, _) = send(
        &router,
        "GET",
        &format!(
            "/api/v1/attachments/{}/preview",
            pdf["id"].as_str().unwrap()
        ),
        &owner,
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/pdf");
    assert!(headers["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("inline"));
    assert_eq!(headers["content-security-policy"], "frame-ancestors 'self'");
    // Other files only download.
    let (status, _, _) = send(
        &router,
        "GET",
        &format!(
            "/api/v1/attachments/{}/preview",
            zip["id"].as_str().unwrap()
        ),
        &owner,
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, headers, _) = send(
        &router,
        "GET",
        &format!("/api/v1/attachments/{}", zip["id"].as_str().unwrap()),
        &owner,
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment"));
    // Ordinary responses still refuse to be framed.
    let (_, headers, _) = send(&router, "GET", "/api/v1/bootstrap", &owner, vec![]).await;
    assert!(headers["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'none'"));
}
