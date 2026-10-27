//! End-to-end AI chat against local mock providers for each supported API dialect.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
mod common;

async fn call(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
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
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn stream(
    router: &Router,
    token: &str,
    chat: &str,
    provider: &str,
    revision: u64,
) -> (StatusCode, Vec<Value>) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/ai/chats/{chat}/messages"))
                .header("content-type", "application/json")
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(
                    json!({"message":"What is the launch codename?","provider_id":provider,"revision":revision}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let events = String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    (status, events)
}

/// Captured request bodies sent to the mock provider.
type Seen = Arc<Mutex<Vec<(String, Value)>>>;

async fn mock_provider(seen: Seen) -> String {
    fn sse(events: &[Value]) -> String {
        events.iter().map(|e| format!("data: {e}\n\n")).collect()
    }
    let record = |path: &'static str, seen: Seen, reply: String| {
        move |body: String| {
            let seen = seen.clone();
            let reply = reply.clone();
            async move {
                seen.lock()
                    .unwrap()
                    .push((path.into(), serde_json::from_str(&body).unwrap()));
                ([("content-type", "text/event-stream")], reply)
            }
        }
    };
    let openai = sse(&[
        json!({"type":"response.output_text.delta","delta":"Codename "}),
        json!({"type":"response.output_text.delta","delta":"Aurora [S1]"}),
        // Web search: a cited page, listed under the answer.
        json!({"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"https://news.example/aurora","title":"Aurora launch"}}),
        json!({"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"javascript:alert(1)","title":"Bad"}}),
        json!({"type":"response.completed","response":{"usage":{"input_tokens":11,"output_tokens":4}}}),
    ]);
    let anthropic = sse(&[
        json!({"type":"message_start","message":{"usage":{"input_tokens":12}}}),
        json!({"type":"content_block_delta","delta":{"type":"citations_delta","citation":{"type":"web_search_result_location","url":"https://wiki.example/Aurora","title":"Aurora [wiki]"}}}),
        json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Aurora [S1]"}}),
        json!({"type":"message_stop"}),
    ]);
    let compatible = format!(
        "{}data: [DONE]\n\n",
        sse(&[
            json!({"choices":[{"delta":{"content":"It is "}}]}),
            json!({"choices":[{"delta":{"content":"Aurora [S1]"}}]}),
            json!({"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":3}}),
        ])
    );
    // A thinking model (Qwen, DeepSeek R1) streams reasoning first, then the
    // answer, and here runs out of length before finishing.
    let thinking = sse(&[
        json!({"choices":[{"delta":{"reasoning_content":"Let me check the notes..."}}]}),
        json!({"choices":[{"delta":{"content":"The codename is "}}]}),
        json!({"choices":[{"delta":{"content":"Aurora"},"finish_reason":"length"}]}),
    ]);
    let overthinking = sse(&[
        json!({"choices":[{"delta":{"reasoning_content":"Hmm, long reasoning..."}}]}),
        json!({"choices":[{"delta":{},"finish_reason":"length"}]}),
    ]);
    let router = Router::new()
        .route(
            "/thinking/chat/completions",
            post(record("thinking", seen.clone(), thinking)),
        )
        .route(
            "/overthinking/chat/completions",
            post(record("overthinking", seen.clone(), overthinking)),
        )
        .route(
            "/openai/responses",
            post(record("openai", seen.clone(), openai)),
        )
        .route(
            "/anthropic/messages",
            post(record("anthropic", seen.clone(), anthropic)),
        )
        .route(
            "/compatible/chat/completions",
            post(record("compatible", seen.clone(), compatible)),
        )
        .route(
            "/slow/chat/completions",
            post(|| async {
                // One delta, then never finishes: the client disconnects.
                let first = futures_util::stream::once(async {
                    Ok::<_, std::io::Error>(axum::body::Bytes::from(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
                    ))
                });
                let body = Body::from_stream(futures_util::StreamExt::chain(
                    first,
                    futures_util::stream::pending(),
                ));
                ([("content-type", "text/event-stream")], body)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{address}")
}

#[tokio::test]
async fn chat_streams_from_each_provider_saves_history_and_releases_on_cancel() {
    let seen: Seen = Arc::default();
    let base = mock_provider(seen.clone()).await;

    let router = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, login) = call(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":"test-password-1234"}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let admin = common::member(&router, &root, "owner@test.local").await;
    let (_, boot) = call(&router, "GET", "bootstrap", &admin, json!({})).await;
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, note) = call(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Launch plan"}),
    )
    .await;
    let mut save = note.clone();
    save["blocks"] = json!([{"id":"b1","markdown":"The launch codename is Aurora."}]);
    call(
        &router,
        "PUT",
        &format!("notes/{}", note["id"].as_str().unwrap()),
        &admin,
        save,
    )
    .await;
    let (_, secret_folder) = call(
        &router,
        "POST",
        "folders",
        &admin,
        json!({"workspace_id":workspace,"name":"Restricted"}),
    )
    .await;
    let (_, secret) = call(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":secret_folder["id"],"title":"Restricted"}),
    )
    .await;
    let mut save = secret.clone();
    save["blocks"] = json!([{"id":"s1","markdown":"Payroll figures must never leave."}]);
    call(
        &router,
        "PUT",
        &format!("notes/{}", secret["id"].as_str().unwrap()),
        &admin,
        save,
    )
    .await;

    let (_, user) = call(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Reader","email":"reader@test.local","password":"test-password-1234"}),
    )
    .await;
    call(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":folder,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let (_, login) = call(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"reader@test.local","password":"test-password-1234"}),
    )
    .await;
    let reader = login["token"].as_str().unwrap().to_string();

    for (kind, path, expected) in [
        ("openai", "openai", "Codename Aurora [S1]\n\n**Web sources**\n- [Aurora launch](https://news.example/aurora)"),
        ("anthropic", "anthropic", "Aurora [S1]\n\n**Web sources**\n- [Aurora wiki](https://wiki.example/Aurora)"),
        ("compatible", "compatible", "It is Aurora [S1]"),
    ] {
        // Web search where the provider offers it.
        let search = kind != "compatible";
        let (status, provider) = call(&router, "POST", "ai/providers", &root, json!({"kind":kind,"name":kind,"endpoint":format!("{base}/{path}"),"model":"mock-model","api_key":format!("key-{kind}"),"web_search":search})).await;
        assert_eq!(status, StatusCode::OK, "{provider}");
        let provider = provider["id"].as_str().unwrap().to_string();
        let (_, chat) = call(
            &router,
            "POST",
            "ai/chats",
            &reader,
            json!({"scope":{"kind":"workspace","id":workspace}}),
        )
        .await;
        let chat = chat["id"].as_str().unwrap().to_string();

        let (status, events) = stream(&router, &reader, &chat, &provider, 0).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(events[0]["type"], "context");
        let sources = events[0]["sources"].as_array().unwrap();
        assert!(sources.iter().any(|s| s["id"] == note["id"]));
        assert!(
            sources.iter().all(|s| s["id"] != secret["id"]),
            "only permitted notes are sources"
        );
        let answer: String = events
            .iter()
            .filter(|e| e["type"] == "delta")
            .map(|e| e["text"].as_str().unwrap())
            .collect();
        assert_eq!(answer, expected);
        assert_eq!(events.last().unwrap()["type"], "done", "{events:?}");
        assert!(events.last().unwrap()["usage"].is_object());

        let (path_seen, sent) = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(path_seen, kind);
        let sent = sent.to_string();
        assert!(
            sent.contains("The launch codename is Aurora."),
            "permitted content reaches the provider"
        );
        assert!(
            !sent.contains("Payroll"),
            "content outside the user's permissions is never sent"
        );
        assert!(sent.contains("What is the launch codename?"));
        // Each provider gets the instructions, and notes come with the link the
        // model copies to cite them.
        assert!(sent.contains("You are the assistant inside Minerva"));
        assert_eq!(sent.contains("\"tools\""), search, "{sent}");
        assert!(
            sent.contains(&format!(
                "cite as [Launch plan](#note/{})",
                note["id"].as_str().unwrap()
            )),
            "{sent}"
        );

        let (_, saved) = call(
            &router,
            "GET",
            &format!("ai/chats/{chat}"),
            &reader,
            json!({}),
        )
        .await;
        assert_eq!(saved["turns"].as_array().unwrap().len(), 1);
        assert_eq!(saved["turns"][0]["answer"], expected);
        assert!(saved["turns"][0]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == note["id"]));
        // A stale revision is rejected instead of interleaving conversations.
        assert_eq!(
            stream(&router, &reader, &chat, &provider, 0).await.0,
            StatusCode::CONFLICT
        );
        // History is sent with the next turn.
        let (status, _) = stream(&router, &reader, &chat, &provider, 1).await;
        assert_eq!(status, StatusCode::OK);
        let (_, sent) = seen.lock().unwrap().last().cloned().unwrap();
        assert!(
            sent.to_string()
                .contains(json!(expected).to_string().trim_matches('"')),
            "previous answer included as history"
        );
    }

    // A thinking model cut off by its length limit: the reasoning is not part
    // of the answer, the written part is kept with a warning, and a model that
    // only reasoned gets a clear error.
    for (path, kept) in [("thinking", true), ("overthinking", false)] {
        // Thinking models get a larger answer limit, sent with every request.
        let (_, p) = call(&router, "POST", "ai/providers", &root, json!({"kind":"compatible","name":path,"endpoint":format!("{base}/{path}"),"model":"qwen","api_key":"k","max_tokens":16384})).await;
        assert_eq!(p["max_tokens"], 16384, "{p}");
        let p = p["id"].as_str().unwrap().to_string();
        let (_, chat) = call(
            &router,
            "POST",
            "ai/chats",
            &reader,
            json!({"scope":{"kind":"workspace","id":workspace}}),
        )
        .await;
        let chat = chat["id"].as_str().unwrap().to_string();
        let (_, events) = stream(&router, &reader, &chat, &p, 0).await;
        assert!(events.iter().any(|e| e["type"] == "thinking"), "{events:?}");
        let (_, sent) = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(sent["max_tokens"], 16384);
        let answer: String = events
            .iter()
            .filter(|e| e["type"] == "delta")
            .map(|e| e["text"].as_str().unwrap())
            .collect();
        assert!(
            !answer.contains("Let me check"),
            "reasoning is not shown as the answer"
        );
        let (_, saved) = call(
            &router,
            "GET",
            &format!("ai/chats/{chat}"),
            &reader,
            json!({}),
        )
        .await;
        if kept {
            assert_eq!(events.last().unwrap()["type"], "done", "{events:?}");
            assert!(
                answer.starts_with("The codename is Aurora") && answer.contains("cut off"),
                "{answer}"
            );
            assert_eq!(saved["turns"][0]["answer"], answer.as_str());
        } else {
            let last = events.last().unwrap();
            assert_eq!(last["type"], "error");
            assert!(
                last["message"].as_str().unwrap().contains("length limit"),
                "{last}"
            );
            assert!(saved["turns"].as_array().unwrap().is_empty());
        }
    }

    // Cancellation: dropping the response stops the request and frees the user's slot
    // without saving partial output.
    let (_, slow) = call(&router, "POST", "ai/providers", &root, json!({"kind":"compatible","name":"slow","endpoint":format!("{base}/slow"),"model":"mock","api_key":"k"})).await;
    let slow = slow["id"].as_str().unwrap().to_string();
    let (_, chat) = call(
        &router,
        "POST",
        "ai/chats",
        &reader,
        json!({"scope":{"kind":"note","id":note["id"]}}),
    )
    .await;
    let chat = chat["id"].as_str().unwrap().to_string();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/ai/chats/{chat}/messages"))
                .header("content-type", "application/json")
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {reader}"))
                .body(Body::from(
                    json!({"message":"Hello","provider_id":slow,"revision":0}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body();
    let mut received = String::new();
    while !received.contains("partial") {
        let frame = body.frame().await.unwrap().unwrap();
        if let Ok(data) = frame.into_data() {
            received.push_str(&String::from_utf8_lossy(&data));
        }
    }
    drop(body);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (_, saved) = call(
        &router,
        "GET",
        &format!("ai/chats/{chat}"),
        &reader,
        json!({}),
    )
    .await;
    assert!(
        saved["turns"].as_array().unwrap().is_empty(),
        "partial output is not saved"
    );
    let compatible = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(k, _)| k == "compatible")
        .count();
    assert!(compatible > 0);
    // The slot was released: a new request is accepted rather than rejected as already running.
    let (_, providers) = call(&router, "GET", "ai/providers", &reader, json!({})).await;
    let fast = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "compatible")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        stream(&router, &reader, &chat, &fast, 0).await.0,
        StatusCode::OK
    );
}
