//! Link previews are only for signed-in people and public web addresses: the
//! server never fetches its own network for them.
use axum::{body::Body, http::Request, Router};
use minerva_server::{app, Store};
use serde_json::json;
use tower::ServiceExt;
mod common;

async fn preview(router: &Router, token: &str, url: &str) -> u16 {
    let uri = format!(
        "/api/v1/link-preview?url={}",
        url::form_urlencoded::byte_serialize(url.as_bytes()).collect::<String>()
    );
    let mut request = Request::builder().uri(uri);
    if !token.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn previews_refuse_private_addresses() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/login")
                .header("content-type", "application/json")
                .header("x-minerva-client", "1")
                .body(Body::from(
                    json!({"email":"admin@test.local","password":common::PASSWORD}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let root = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let token = common::member(&router, &root, "member@test.local").await;

    assert_eq!(preview(&router, "", "https://example.com").await, 401);
    for refused in [
        "http://127.0.0.1/",
        "http://localhost/",
        "http://[::1]/",
        "http://169.254.169.254/latest/meta-data/",
        "http://10.0.0.1/",
        "http://192.168.1.1/",
        "https://example.com:8443/",
        "ftp://example.com/",
        "file:///etc/passwd",
        "not a url",
    ] {
        assert_eq!(preview(&router, &token, refused).await, 400, "{refused}");
    }
}
