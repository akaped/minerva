//! End-to-end AI chat against local mock providers for each supported API dialect.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
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

async fn mock(
    shell: Option<&'static str>,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let seen = Arc::new(Mutex::new(vec![]));
    let records = seen.clone();
    let router = Router::new()
        .route("/partial/models", get(|| async {axum::Json(json!({"data":[{"id":"vendor/model"}]}))}))
        .route("/models", get(|headers: axum::http::HeaderMap| async move {
            assert_eq!(headers["authorization"], "Bearer test-key");
            axum::Json(json!({"data":[{"id":"vendor/model","name":"Test model","context_length":128000,"architecture":{"input_modalities":["text","image"],"output_modalities":["text"]},"supported_parameters":["tools"]}]}))
        }))
        .route("/tools",get(|| async {axum::Json(json!({"data":[{"id":"openrouter:datetime","name":"Datetime","status":"active","supported_api_formats":["responses"]},{"id":"bad:tool","status":"active","supported_api_formats":["responses"]}]}))}))
        .route("/responses",post(move |axum::Json(body):axum::Json<Value>| {
            let records=records.clone();
            async move {
                records.lock().unwrap().push(body.clone());
                let continued = body["input"].as_array().unwrap().iter().any(|i| i["type"]=="function_call_output");
                let events = if shell.is_some() && !continued {
                    vec![json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":"call-1","name":"minerva_shell","arguments":json!({"command":shell.unwrap()}).to_string()}}),json!({"type":"response.completed","response":{"usage":{"input_tokens":1}}})]
                } else {
                    vec![json!({"type":"response.output_text.delta","delta":"A useful answer"}),json!({"type":"response.output_item.done","item":{"type":"openrouter:image_generation","status":"completed","imageUrl":"data:image/png;base64,aGVsbG8="}}),json!({"type":"response.completed","response":{"usage":{"input_tokens":12,"output_tokens":3}}})]
                };
                ([("content-type","text/event-stream")],events.iter().map(|e|format!("data: {e}\n\n")).collect::<String>())
            }
        }));
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", socket.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(socket, router).await.unwrap() });
    (endpoint, seen, task)
}
async fn create(router: &Router, token: &str, endpoint: &str, tools: Value) -> Value {
    let (status,p)=call(router,"POST","ai/providers",token,json!({"kind":"openrouter","endpoint":endpoint,"name":"Router","model":"vendor/model","api_key":"test-key","server_tools":tools})).await;
    assert_eq!(status, StatusCode::OK, "{p}");
    p
}
async fn chat(router: &Router, token: &str) -> String {
    let (status, c) = call(
        router,
        "POST",
        "ai/chats",
        token,
        json!({"scope":{"kind":"none","id":""}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{c}");
    c["id"].as_str().unwrap().into()
}
#[tokio::test]
async fn discovery_profiles_and_openrouter_responses() {
    let (endpoint, seen, task) = mock(None).await;
    let router = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, login) = call(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":"test-password-1234"}),
    )
    .await;
    let token = login["token"].as_str().unwrap();
    let p = create(
        &router,
        token,
        &endpoint,
        json!(["openrouter:datetime", "openrouter:image_generation"]),
    )
    .await;
    assert!(!p.to_string().contains("test-key"));
    let discovery = json!({"kind":"openrouter","endpoint":endpoint,"provider_id":p["id"]});
    let (status, result) = call(
        &router,
        "POST",
        "ai/providers/discover",
        token,
        discovery.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["models"][0]["context_length"], 128000);
    assert_eq!(result["tools"].as_array().unwrap().len(), 1);
    let (status, partial) = call(
        &router,
        "POST",
        "ai/providers/discover",
        token,
        json!({"kind":"openrouter","endpoint":format!("{endpoint}/partial"),"api_key":"test-key"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(partial["models"][0]["id"], "vendor/model");
    assert_eq!(partial["tools_available"], false);
    let mut wrong = discovery.clone();
    wrong["endpoint"] = json!(format!("{endpoint}/other"));
    assert_eq!(
        call(&router, "POST", "ai/providers/discover", token, wrong)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let member = common::member(&router, token, "member@test.local").await;
    assert_eq!(
        call(&router, "POST", "ai/providers/discover", &member, discovery)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &router,
            "PATCH",
            &format!("ai/providers/{}", p["id"].as_str().unwrap()),
            &member,
            p.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let c = chat(&router, &member).await;
    let (_, events) = stream(&router, &member, &c, p["id"].as_str().unwrap(), 0).await;
    assert!(events.iter().any(|e| e["type"] == "done"), "{events:?}");
    assert!(events.iter().any(|e| e["type"] == "image"));
    let payload = seen.lock().unwrap()[0].clone();
    assert_eq!(
        payload["tools"],
        json!([{"type":"openrouter:datetime"},{"type":"openrouter:image_generation"}])
    );
    assert_eq!(payload["max_tool_calls"], 8);
    let (_, saved) = call(&router, "GET", &format!("ai/chats/{c}"), &member, json!({})).await;
    assert_eq!(
        saved["turns"][0]["images"][0],
        "data:image/png;base64,aGVsbG8="
    );
    let mut edit = p.clone();
    edit["server_tools"] = json!([]);
    assert_eq!(
        call(
            &router,
            "PATCH",
            &format!("ai/providers/{}", p["id"].as_str().unwrap()),
            token,
            edit
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, events) = stream(&router, &member, &c, p["id"].as_str().unwrap(), 1).await;
    assert!(events.iter().any(|e| e["type"] == "done"));
    assert!(seen.lock().unwrap()[1].get("tools").is_none());
    for tools in [
        json!(["minerva:shell"]),
        json!(["unknown"]),
        json!(["openrouter:fusion"]),
    ] {
        let (status,_)=call(&router,"POST","ai/providers",token,json!({"kind":"openrouter","endpoint":endpoint,"name":"Bad","model":"x","api_key":"key","server_tools":tools})).await;
        assert!(!status.is_success());
    }
    assert_eq!(
        call(&router, "GET", "ai/shell/settings", token, json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    task.abort();
}

#[tokio::test]
async fn desktop_shell_requires_approval_and_supports_unattended_mode() {
    let (endpoint, seen, task) = mock(Some("echo minerva-shell-ok")).await;
    let (store, token) = Store::open_local(":memory:").unwrap();
    let router = app(store);
    let (_, settings) = call(&router, "GET", "ai/shell/settings", &token, json!({})).await;
    assert_eq!(settings["unattended"], false);
    let directory = std::env::temp_dir().to_string_lossy().to_string();
    assert_eq!(
        call(
            &router,
            "PUT",
            "ai/shell/settings",
            &token,
            json!({"directory":directory,"unattended":false})
        )
        .await
        .0,
        StatusCode::OK
    );
    let p = create(&router, &token, &endpoint, json!(["minerva:shell"])).await;
    for approve in [false, true] {
        let c = chat(&router, &token).await;
        let (r, t, id, pid) = (
            router.clone(),
            token.clone(),
            c.clone(),
            p["id"].as_str().unwrap().to_string(),
        );
        let running = tokio::spawn(async move { stream(&r, &t, &id, &pid, 0).await });
        let pending = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let (_, pending) = call(
                    &router,
                    "GET",
                    &format!("ai/shell/pending/{c}"),
                    &token,
                    json!({}),
                )
                .await;
                if !pending.as_array().unwrap().is_empty() {
                    break pending;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!running.is_finished());
        assert_eq!(pending[0]["command"], "echo minerva-shell-ok");
        let key = pending[0]["id"].as_str().unwrap();
        assert_eq!(
            call(
                &router,
                "POST",
                &format!("ai/shell/approval/{key}"),
                &token,
                json!({"approved":approve})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &router,
                "POST",
                &format!("ai/shell/approval/{key}"),
                &token,
                json!({"approved":true})
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        let (_, events) = running.await.unwrap();
        assert!(events.iter().any(|e| e["type"] == "done"), "{events:?}");
        let last = seen.lock().unwrap().last().unwrap().clone();
        let result = last["input"].as_array().unwrap().last().unwrap()["output"]
            .as_str()
            .unwrap();
        if approve {
            assert!(result.contains("minerva-shell-ok"), "{result}");
        } else {
            assert!(result.contains("denied"), "{result}");
        }
    }
    assert_eq!(
        call(
            &router,
            "PUT",
            "ai/shell/settings",
            &token,
            json!({"directory":directory,"unattended":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    let c = chat(&router, &token).await;
    let (_, events) = stream(&router, &token, &c, p["id"].as_str().unwrap(), 0).await;
    assert!(events.iter().any(|e| e["type"] == "done"));
    task.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn cancelling_desktop_shell_stops_the_tool_loop() {
    let (endpoint, seen, task) = mock(Some("sleep 10")).await;
    let (store, token) = Store::open_local(":memory:").unwrap();
    let router = app(store);
    let directory = std::env::temp_dir().to_string_lossy().to_string();
    call(
        &router,
        "PUT",
        "ai/shell/settings",
        &token,
        json!({"directory":directory,"unattended":true}),
    )
    .await;
    let p = create(&router, &token, &endpoint, json!(["minerva:shell"])).await;
    let c = chat(&router, &token).await;
    let (r, t, id, pid) = (
        router.clone(),
        token.clone(),
        c.clone(),
        p["id"].as_str().unwrap().to_string(),
    );
    let running = tokio::spawn(async move { stream(&r, &t, &id, &pid, 0).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if !seen.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(
        call(
            &router,
            "POST",
            &format!("ai/shell/cancel/{c}"),
            &token,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, events) = tokio::time::timeout(std::time::Duration::from_secs(3), running)
        .await
        .unwrap()
        .unwrap();
    assert!(events.iter().any(|e| e["type"] == "error"));
    assert!(!events.iter().any(|e| e["type"] == "done"));
    assert_eq!(seen.lock().unwrap().len(), 1);
    task.abort();
}
