//! Long recordings go to transcription in parts, and speakers come along.
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
async fn upload(r: &Router, token: &str, note: &str, name: &str, bytes: &[u8]) -> Value {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/notes/{note}/attachments?filename={name}"))
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(bytes.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn finished(r: &Router, token: &str, id: &str) -> Value {
    for _ in 0..200 {
        let (status, job) = call(r, "GET", &format!("ai/recordings/{id}"), token, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{job}");
        if !["transcribing", "summarizing"].contains(&job["state"].as_str().unwrap()) {
            return job;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("The job did not finish")
}

#[tokio::test]
async fn long_recordings_go_in_parts_with_speakers() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let mock = Router::new().route(
        "/asr/audio/transcriptions",
        post(|body: axum::body::Bytes| async move {
            let form = String::from_utf8_lossy(&body);
            assert!(form.contains("diarized_json") && form.contains("chunking_strategy"));
            assert!(body.len() <= 25 * 1024 * 1024, "each part fits");
            let n = CALLS.fetch_add(1, Ordering::SeqCst) + 1;
            axum::Json(json!({
                "duration": 600.0,
                "segments": [{"start": 1.0, "end": 2.0, "text": format!("Part {n}"), "speaker": if n == 1 { "A" } else { "B" }}]
            }))
        }),
    );
    let mock = mock.layer(axum::extract::DefaultBodyLimit::max(30 * 1024 * 1024));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });

    let r = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = call(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let admin = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&r, &admin, "owner@test.local").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"].clone();
    let (_, note) = call(
        &r,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Long meeting"}),
    )
    .await;
    let nid = note["id"].as_str().unwrap().to_string();
    // About 30 MB of 16 kHz mono 16-bit WAV: more than one request takes.
    let data = 30 * 1024 * 1024;
    let mut wav = b"RIFF".to_vec();
    wav.extend(((36 + data) as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend([1, 0, 1, 0]);
    wav.extend(16000u32.to_le_bytes());
    wav.extend(32000u32.to_le_bytes());
    wav.extend([2, 0, 16, 0]);
    wav.extend(b"data");
    wav.extend((data as u32).to_le_bytes());
    wav.extend(vec![0u8; data]);
    let audio = upload(&r, &owner, &nid, "Meeting.wav", &wav).await;
    let (_, asr) = call(&r, "POST", "ai/providers", &admin, json!({"name": "asr", "kind": "compatible", "endpoint": format!("http://{address}/asr"), "model": "mock", "api_key": "secret"})).await;
    std::env::set_var("MINERVA_ASR_PROVIDER", asr["id"].as_str().unwrap());
    std::env::set_var("MINERVA_ASR_MODEL", "gpt-4o-transcribe-diarize");
    let (status, job) = call(
        &r,
        "POST",
        "ai/recordings",
        &owner,
        json!({"note_id": nid, "attachment_id": audio["id"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let done = finished(&r, &owner, job["id"].as_str().unwrap()).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["parts"], 2);
    assert_eq!(
        done["transcript"],
        json!([
            {"start": 1.0, "end": 2.0, "text": "Part 1", "speaker": "A"},
            {"start": 601.0, "end": 602.0, "text": "Part 2", "speaker": "B"}
        ]),
        "the second part's times follow the first's"
    );
}
