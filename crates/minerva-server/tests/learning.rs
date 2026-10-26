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
async fn wait(r: &Router, token: &str, id: &str) -> Value {
    for _ in 0..200 {
        let (status, j) = call(
            r,
            "GET",
            &format!("server-plugins/jobs/{id}"),
            token,
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        if !["queued", "running"].contains(&j["status"].as_str().unwrap()) {
            return j;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("Job did not finish")
}
#[tokio::test]
async fn learning_end_to_end_and_failure_boundaries() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    std::env::set_var("MINERVA_PLUGINS", &root);
    let result=json!({"items":[{"kind":"quote","text":"Feedback improves learning.","source_ids":["b1"],"grounding":"extraction"}]}).to_string();
    let compatible = result.clone();
    let openai = result.clone();
    let anthropic = result.clone();
    let mock=Router::new()
 .route("/compatible/chat/completions",post(move ||{let text=compatible.clone();async move{axum::Json(json!({"choices":[{"message":{"content":text},"finish_reason":"stop"}]}))}}))
 .route("/openai/responses",post(move ||{let text=openai.clone();async move{axum::Json(json!({"status":"completed","output":[{"content":[{"type":"output_text","text":text}]}]}))}}))
 .route("/anthropic/messages",post(move ||{let text=anthropic.clone();async move{axum::Json(json!({"stop_reason":"end_turn","content":[{"text":text}]}))}}))
 .route("/bad/chat/completions",post(||async{axum::Json(json!({"choices":[{"message":{"content":"not JSON"},"finish_reason":"stop"}]}))}))
 .route("/asr/audio/transcriptions",post(|body: axum::body::Bytes| async move {assert!(String::from_utf8_lossy(&body).contains("verbose_json"));axum::Json(json!({"language":"en","segments":[{"start":1.25,"end":3.75,"text":"Feedback improves learning."}]}))}))
 .route("/slow/chat/completions",post(||async{tokio::time::sleep(std::time::Duration::from_secs(30)).await;axum::Json(json!({}))}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, login) = call(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":"test-password-1234"}),
    )
    .await;
    let admin_token = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&r, &admin_token, "owner@test.local").await;
    let token = owner.as_str();
    let (_, boot) = call(&r, "GET", "bootstrap", token, json!({})).await;
    let (_, note) = call(
        &r,
        "POST",
        "notes",
        token,
        json!({"folder_id":common::team_folder(&boot)["id"],"title":"Learning"}),
    )
    .await;
    let note_id = note["id"].as_str().unwrap();
    let mut saved = note.clone();
    saved["blocks"] = json!([{"id":"b1","markdown":"Feedback improves learning."}]);
    assert_eq!(
        call(&r, "PUT", &format!("notes/{note_id}"), token, saved)
            .await
            .0,
        StatusCode::OK
    );
    let (_, caps) = call(&r, "GET", "server-plugins/capabilities", token, json!({})).await;
    assert_eq!(caps["commands"][0]["available"], false);
    let mut last = String::new();
    let mut last_input = Value::Null;
    for (kind, path) in [
        ("compatible", "compatible"),
        ("openai", "openai"),
        ("anthropic", "anthropic"),
        ("compatible", "bad"),
        ("compatible", "slow"),
    ] {
        let (_,p)=call(&r,"POST","ai/providers",&admin_token,json!({"name":path,"kind":kind,"endpoint":format!("http://{address}/{path}"),"model":"mock","api_key":"secret"})).await;
        let input = json!({"plugin_id":"superhuman-learning","command":"wisdom","note_id":note_id,"provider_id":p["id"]});
        let (status, j) = call(&r, "POST", "server-plugins/jobs", token, input.clone()).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{j}");
        let key = j["id"].as_str().unwrap();
        last = key.into();
        last_input = input.clone();
        if path == "slow" {
            assert_eq!(
                call(&r, "POST", "server-plugins/jobs", token, input)
                    .await
                    .0,
                StatusCode::TOO_MANY_REQUESTS
            );
            let (_, j) = call(
                &r,
                "POST",
                &format!("server-plugins/jobs/{key}"),
                token,
                json!({"action":"cancel"}),
            )
            .await;
            assert_eq!(j["status"], "cancelled");
        } else {
            let done = wait(&r, token, key).await;
            if path == "bad" {
                assert_eq!(done["status"], "failed");
                assert!(done["result"].is_null());
            } else {
                assert_eq!(done["status"], "completed", "{done}");
                assert_eq!(done["result"]["items"][0]["source_ids"][0], "b1");
                assert_eq!(done["sources"][0]["text"], "Feedback improves learning.");
            }
        }
    }
    // Audio is an existing authorized attachment; no model credentials enter workers.
    let (_,asr)=call(&r,"POST","ai/providers",&admin_token,json!({"name":"ASR","kind":"compatible","endpoint":format!("http://{address}/asr"),"model":"unused-chat-model","api_key":"secret"})).await;
    std::env::set_var("MINERVA_ASR_PROVIDER", asr["id"].as_str().unwrap());
    std::env::set_var("MINERVA_ASR_MODEL", "mock-whisper");
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/notes/{note_id}/attachments?filename=lecture.wav"
                ))
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(b"RIFF0000WAVEtest".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let attachment: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let audio = json!({"plugin_id":"superhuman-learning","command":"transcribe","note_id":note_id,"attachment_id":attachment["id"]});
    let (status, j) = call(&r, "POST", "server-plugins/jobs", token, audio.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{j}");
    let done = wait(&r, token, j["id"].as_str().unwrap()).await;
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(done["sources"][0]["start"], 1.25);
    assert_eq!(done["result"]["transcript_only"], true);
    let mut retry = audio.clone();
    retry["retry_of"] = j["id"].clone();
    let (status, j) = call(&r, "POST", "server-plugins/jobs", token, retry).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{j}");
    assert_eq!(
        wait(&r, token, j["id"].as_str().unwrap()).await["sources"],
        done["sources"]
    );
    std::env::remove_var("MINERVA_ASR_PROVIDER");
    std::env::remove_var("MINERVA_ASR_MODEL");
    let (_, caps) = call(&r, "GET", "server-plugins/capabilities", token, json!({})).await;
    assert_eq!(
        caps["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "transcribe")
            .unwrap()["available"],
        false
    );
    let invalid_video = json!({"plugin_id":"superhuman-learning","command":"youtube","note_id":note_id,"url":"https://localhost/secret"});
    assert_eq!(
        call(&r, "POST", "server-plugins/jobs", token, invalid_video)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // Input cannot smuggle text unrelated to an authorized source block.
    last_input["block_id"] = json!("b1");
    last_input["selection"] = json!("not in this note");
    assert_eq!(
        call(&r, "POST", "server-plugins/jobs", token, last_input)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &r,
            "GET",
            &format!("server-plugins/jobs/{last}"),
            "",
            json!({})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, user) = call(
        &r,
        "POST",
        "users",
        &admin_token,
        json!({"name":"Reader","email":"reader@test.local","password":"test-password-1234"}),
    )
    .await;
    call(
        &r,
        "PUT",
        "permissions",
        token,
        json!({"folder_id":common::team_folder(&boot)["id"],"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let (_, login) = call(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"reader@test.local","password":"test-password-1234"}),
    )
    .await;
    assert_eq!(
        call(
            &r,
            "GET",
            &format!("server-plugins/jobs/{last}"),
            login["token"].as_str().unwrap(),
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &r,
            "DELETE",
            &format!("server-plugins/jobs/{last}"),
            token,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &r,
            "GET",
            &format!("server-plugins/jobs/{last}"),
            token,
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Removing the plugin root leaves core notes working and no commands.
    std::env::set_var("MINERVA_PLUGINS", root.join("absent"));
    let (_, caps) = call(&r, "GET", "server-plugins/capabilities", token, json!({})).await;
    assert!(caps["commands"].as_array().unwrap().is_empty());
    assert_eq!(
        call(&r, "GET", &format!("notes/{note_id}"), token, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    std::env::remove_var("MINERVA_PLUGINS");
    server.abort();
}
