//! From a recording to notes, against local mock providers: transcribing,
//! summarizing into action items, and who may do it.
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
async fn recordings_become_transcripts_and_action_items() {
    let notes = json!({
        "title": "Launch sync",
        "summary": ["The launch moves to Friday.", "QA needs one more day."],
        "decisions": ["Ship on Friday"],
        "action_items": [
            {"task": "Send the release notes", "owner": "Ada", "due": "2026-11-06 10:00"},
            {"task": "Book the demo room", "owner": "", "due": ""}
        ]
    })
    .to_string();
    let mock = Router::new()
        .route(
            "/asr/audio/transcriptions",
            post(|body: axum::body::Bytes| async move {
                let form = String::from_utf8_lossy(&body);
                // Not a Whisper model: plain text, no timestamps asked for.
                assert!(form.contains("\r\n\r\njson\r\n") && !form.contains("verbose_json"));
                assert!(form.contains("filename=\"audio.webm\""));
                axum::Json(json!({"text": "We move the launch to Friday.\nAda sends the notes."}))
            }),
        )
        .route(
            "/llm/chat/completions",
            post(move |body: axum::Json<Value>| {
                let notes = notes.clone();
                async move {
                    let system = body["messages"][0]["content"].as_str().unwrap_or("");
                    assert!(system.contains("today, which is 2026-11-03"), "{system}");
                    let input = body["messages"][1]["content"].as_str().unwrap_or("");
                    assert!(input.contains("Ada sends the notes."));
                    axum::Json(
                        json!({"choices":[{"message":{"content":notes},"finish_reason":"stop"}]}),
                    )
                }
            }),
        );
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
    let viewer = common::account(&r, &admin, "viewer@test.local").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"].clone();
    let (_, note) = call(
        &r,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Sync"}),
    )
    .await;
    let nid = note["id"].as_str().unwrap().to_string();
    // A browser recording (WebM) and a file that is not audio.
    let audio = upload(
        &r,
        &owner,
        &nid,
        "Recording.webm",
        b"\x1a\x45\xdf\xa3webm-audio",
    )
    .await;
    let text = upload(&r, &owner, &nid, "notes.txt", b"just text").await;
    let start = |attachment: &Value, provider: &Value| json!({"note_id": nid, "attachment_id": attachment["id"], "provider_id": provider, "today": "2026-11-03"});

    // Without a transcription provider, it says what is missing.
    let (_, state) = call(&r, "GET", "ai/recordings", &owner, json!({})).await;
    assert_eq!(state["available"], false);
    let (status, err) = call(
        &r,
        "POST",
        "ai/recordings",
        &owner,
        start(&audio, &Value::Null),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"]
        .as_str()
        .unwrap()
        .contains("MINERVA_ASR_PROVIDER"));

    let provider = |name: &str| {
        json!({"name": name, "kind": "compatible", "endpoint": format!("http://{address}/{name}"),
               "model": "mock", "api_key": "secret"})
    };
    let (_, asr) = call(&r, "POST", "ai/providers", &admin, provider("asr")).await;
    let (_, llm) = call(&r, "POST", "ai/providers", &admin, provider("llm")).await;
    std::env::set_var("MINERVA_ASR_PROVIDER", asr["id"].as_str().unwrap());
    std::env::set_var("MINERVA_ASR_MODEL", "gpt-4o-mini-transcribe");
    let (_, state) = call(&r, "GET", "ai/recordings", &owner, json!({})).await;
    assert_eq!(state["available"], true);

    // Transcript only.
    let (status, job) = call(
        &r,
        "POST",
        "ai/recordings",
        &owner,
        start(&audio, &Value::Null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["state"], "transcribing");
    let done = finished(&r, &owner, job["id"].as_str().unwrap()).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(
        done["transcript"],
        json!([{"text": "We move the launch to Friday."}, {"text": "Ada sends the notes."}])
    );
    assert!(done.get("summary").is_none());

    // With notes: key points, decisions and action items.
    let (_, job) = call(
        &r,
        "POST",
        "ai/recordings",
        &owner,
        start(&audio, &llm["id"]),
    )
    .await;
    let done = finished(&r, &owner, job["id"].as_str().unwrap()).await;
    assert_eq!(done["state"], "done", "{done}");
    assert_eq!(done["summary"]["decisions"], json!(["Ship on Friday"]));
    assert_eq!(
        done["summary"]["action_items"][0]["due"],
        "2026-11-06 10:00"
    );
    assert_eq!(done["summary"]["action_items"][1]["owner"], "");

    // Only the person who started a job sees it; only editors start one.
    let (status, _) = call(
        &r,
        "GET",
        &format!("ai/recordings/{}", job["id"].as_str().unwrap()),
        &viewer,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, vboot) = call(&r, "GET", "bootstrap", &viewer, json!({})).await;
    call(
        &r,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id": folder, "user_id": vboot["user"]["id"], "role": "viewer"}),
    )
    .await;
    let (status, _) = call(
        &r,
        "POST",
        "ai/recordings",
        &viewer,
        start(&audio, &Value::Null),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, err) = call(
        &r,
        "POST",
        "ai/recordings",
        &owner,
        start(&text, &Value::Null),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");

    std::env::remove_var("MINERVA_ASR_PROVIDER");
    std::env::remove_var("MINERVA_ASR_MODEL");
}
