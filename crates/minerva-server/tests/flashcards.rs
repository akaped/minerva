//! Real plugin worker and local mock models; no network services or paid requests.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;
async fn call(r: &Router, method: &str, path: &str, token: &str, v: Value) -> (StatusCode, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
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
async fn login(r: &Router, email: &str) -> String {
    call(
        r,
        "POST",
        "login",
        "",
        json!({"email":email,"password":"test-password-1234"}),
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .into()
}
#[tokio::test]
async fn decks_reviews_persistence_and_isolation() {
    let dir = std::env::temp_dir().join(format!("minerva-flashcards-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.db");
    let r = app(Store::open(
        path.to_str().unwrap(),
        "admin@test.local",
        "test-password-1234",
    )
    .unwrap());
    let root = login(&r, "admin@test.local").await;
    let token = common::member(&r, &root, "owner@test.local").await;
    let (status, d) = call(
        &r,
        "POST",
        "flashcards/decks",
        &token,
        json!({"name":"Biology","description":"Cells"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let route = format!("flashcards/decks/{}", d["id"].as_str().unwrap());
    let (_,r1)=call(&r,"POST",&format!("{route}/cards"),&token,json!({"revision":d["revision"],"cards":[{"front":"What stores genetic information?","back":"DNA","sources":[]}]})).await;
    let card = &r1["deck"]["cards"][0];
    let card_route = format!("{route}/cards/{}", card["id"].as_str().unwrap());
    let (_, summary) = call(&r, "GET", "flashcards/decks", &token, json!({})).await;
    assert_eq!(summary["decks"][0]["due"], 1);
    let (status, r2) = call(
        &r,
        "POST",
        &format!("{card_route}/review"),
        &token,
        json!({"revision":card["revision"],"rating":"good"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(r2["deck"]["cards"][0]["interval"], 86400);
    assert_eq!(
        call(
            &r,
            "POST",
            &format!("{card_route}/review"),
            &token,
            json!({"revision":card["revision"],"rating":"good"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &r,
            "POST",
            &format!("{card_route}/review"),
            &token,
            json!({"revision":1,"rating":"good"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, summary) = call(&r, "GET", "flashcards/decks", &token, json!({})).await;
    assert_eq!(summary["decks"][0]["due"], 0);
    call(
        &r,
        "POST",
        "users",
        &root,
        json!({"name":"Member","email":"m@test.local","password":"test-password-1234"}),
    )
    .await;
    let member = login(&r, "m@test.local").await;
    assert_eq!(
        call(&r, "GET", &route, &member, json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &r,
            "DELETE",
            &route,
            &member,
            json!({"revision":r2["deck"]["revision"]})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, summary) = call(&r, "GET", "flashcards/decks", &member, json!({})).await;
    assert_eq!(summary["decks"], json!([]));
    assert_eq!(call(&r,"POST",&format!("{route}/cards"),&token,json!({"revision":r2["deck"]["revision"],"cards":[{"front":"bad link","back":"answer","sources":["javascript:alert(1)"]}]})).await.0,StatusCode::BAD_REQUEST);
    drop(r);
    let r = app(Store::open(
        path.to_str().unwrap(),
        "admin@test.local",
        "test-password-1234",
    )
    .unwrap());
    let root = login(&r, "admin@test.local").await;
    let token = common::member(&r, &root, "owner@test.local").await;
    let (_, loaded) = call(&r, "GET", &route, &token, json!({})).await;
    assert_eq!(loaded["deck"]["cards"][0]["reviews"], 1);
    let mut edited = loaded["deck"]["cards"][0].clone();
    edited["front"] = json!("Edited question");
    edited["suspended"] = json!(true);
    let (status, updated) = call(&r, "PUT", &card_route, &token, edited).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["deck"]["cards"][0]["suspended"], true);
    assert_eq!(
        call(&r, "DELETE", &card_route, &token, json!({"revision":2}))
            .await
            .0,
        StatusCode::OK
    );
    let (_, loaded) = call(&r, "GET", &route, &token, json!({})).await;
    assert_eq!(loaded["deck"]["cards"], json!([]));
    assert_eq!(
        call(
            &r,
            "DELETE",
            &route,
            &token,
            json!({"revision":loaded["deck"]["revision"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    drop(r);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn generation_uses_authorized_notes_and_real_search_sources() {
    let mock=Router::new().route("/v1/chat/completions",post(|axum::Json(v):axum::Json<Value>|async move {
        assert!(v.get("tools").is_none());
        let input=v["messages"][1]["content"].as_str().unwrap();
        let source=input.split_whitespace().find(|s|s.starts_with("#note/")).unwrap().trim_end_matches(':');
        let cards=json!({"cards":[{"front":"What is stored in DNA?","back":"Genetic information.","sources":[source]}]}).to_string();
        axum::Json(json!({"choices":[{"message":{"content":cards},"finish_reason":"stop"}]}))
    })).route("/research/responses",post(|axum::Json(v):axum::Json<Value>|async move {
        assert_eq!(v["tools"][0]["type"],"web_search");assert_eq!(v["include"][0],"web_search_call.action.sources");
        let text=json!({"cards":[{"front":"What is DNA?","back":"A molecule carrying genetic information.","sources":["https://example.org/dna"]}]}).to_string();
        axum::Json(json!({"status":"completed","output":[{"type":"web_search_call","action":{"sources":[{"url":"https://example.org/dna"}]}},{"content":[{"type":"output_text","text":text}]}]}))
    })).route("/fake/responses",post(||async {
        let text=json!({"cards":[{"front":"Question","back":"Answer","sources":["https://invented.example/source"]}]}).to_string();
        axum::Json(json!({"status":"completed","output":[{"content":[{"type":"output_text","text":text}]}]}))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&r, "admin@test.local").await;
    let token = common::member(&r, &root, "owner@test.local").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &token, json!({})).await;
    let note = &common::welcome(&boot);
    let (_,p)=call(&r,"POST","ai/providers",&root,json!({"name":"mock","kind":"compatible","endpoint":format!("http://{address}/v1"),"model":"mock","api_key":"test"})).await;
    for kind in ["note", "folder"] {
        let (status,out)=call(&r,"POST","flashcards/generate",&token,json!({"kind":kind,"source_id":if kind=="note" {&note["id"]}else{&note["folder_id"]},"provider_id":p["id"],"count":5})).await;
        assert_eq!(status, StatusCode::OK, "{out}");
        assert_eq!(out["cards"].as_array().unwrap().len(), 1);
    }
    let (_, decks) = call(&r, "GET", "flashcards/decks", &token, json!({})).await;
    assert_eq!(decks["decks"], json!([]), "generation must not save drafts");
    assert_eq!(
        call(
            &r,
            "POST",
            "flashcards/generate",
            &token,
            json!({"kind":"topic","topic":"DNA","provider_id":p["id"],"count":5})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for (path, expected) in [
        ("research", StatusCode::OK),
        ("fake", StatusCode::BAD_REQUEST),
    ] {
        let (_,p)=call(&r,"POST","ai/providers",&root,json!({"name":path,"kind":"openai","endpoint":format!("http://{address}/{path}"),"model":"mock","api_key":"test","web_search":true})).await;
        let (status, result) = call(
            &r,
            "POST",
            "flashcards/generate",
            &token,
            json!({"kind":"topic","topic":"DNA","provider_id":p["id"],"count":5}),
        )
        .await;
        assert_eq!(status, expected, "{result}");
    }
    call(
        &r,
        "POST",
        "users",
        &root,
        json!({"name":"Other","email":"other@test.local","password":"test-password-1234"}),
    )
    .await;
    let other = login(&r, "other@test.local").await;
    assert_eq!(
        call(
            &r,
            "POST",
            "flashcards/generate",
            &other,
            json!({"kind":"note","source_id":note["id"],"provider_id":p["id"],"count":5})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    server.abort();
}
