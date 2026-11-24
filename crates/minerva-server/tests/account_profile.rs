use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;

mod common;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
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
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

fn png_avatar(width: u32) -> String {
    let mut image = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut image, width, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![42; width as usize * 3])
            .unwrap();
    }
    format!("data:image/png;base64,{}", STANDARD.encode(image))
}

#[tokio::test]
async fn a_member_can_edit_their_name_and_picture() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, admin) = call(
        &router,
        "POST",
        "login",
        "",
        json!({
            "email": "admin@test.local", "password": common::PASSWORD
        }),
    )
    .await;
    let admin = admin["token"].as_str().unwrap();
    let member = common::account(&router, admin, "member@test.local").await;
    let other = common::account(&router, admin, "other@test.local").await;

    let (_, start) = call(&router, "GET", "bootstrap", &member, json!({})).await;
    assert_eq!(start["profile"]["first_name"], start["user"]["name"]);
    assert!(start["profile"]["avatar"].is_null());

    let portrait = png_avatar(128);
    for avatar in [json!("preset:fox"), json!(portrait)] {
        let (status, saved) = call(
            &router,
            "PATCH",
            "account/profile",
            &member,
            json!({
                "first_name": " Ada ", "last_name": " Lovelace ", "avatar": avatar,
                "email": "replacement@test.local", "admin": true
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{saved}");
        assert_eq!(saved["user"]["name"], "Ada Lovelace");
        assert_eq!(saved["user"]["email"], "member@test.local");
        assert_eq!(saved["user"]["admin"], false);
        assert_eq!(saved["profile"]["avatar"], avatar);
    }
    let (_, read) = call(&router, "GET", "bootstrap", &member, json!({})).await;
    assert_eq!(read["profile"]["avatar"], portrait);
    let (_, read_other) = call(&router, "GET", "bootstrap", &other, json!({})).await;
    assert!(read_other["profile"]["avatar"].is_null());

    for invalid in [
        "data:image/png;base64,AA==".into(),
        "preset:unknown".into(),
        png_avatar(129),
    ] {
        let (status, _) = call(
            &router,
            "PATCH",
            "account/profile",
            &member,
            json!({
                "first_name": "Wrong", "last_name": "", "avatar": invalid
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (_, unchanged) = call(&router, "GET", "bootstrap", &member, json!({})).await;
    assert_eq!(unchanged["user"]["name"], "Ada Lovelace");
    assert_eq!(unchanged["profile"]["avatar"], portrait);

    let (status, reset) = call(
        &router,
        "PATCH",
        "account/profile",
        &member,
        json!({
            "first_name": "Ada", "last_name": "", "avatar": null
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset["user"]["name"], "Ada");
    assert!(reset["profile"]["avatar"].is_null());
}
