//! MCP write tools: only credentials created with write access list and call
//! them, always within the user's edit permissions, and changes land like edits
//! made in the app (history, revisions, validated database values).
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn send(router: &Router, method: &str, uri: &str, token: &str, body: Value) -> (u16, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    send(router, method, &format!("/api/v1/{path}"), token, body).await
}
/// Calls a tool: whether it failed, and its result.
async fn call(router: &Router, wid: &str, token: &str, name: &str, args: Value) -> (bool, Value) {
    let (status, v) = send(
        router,
        "POST",
        &format!("/api/workspaces/{wid}/mcp"),
        token,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
    )
    .await;
    assert_eq!(status, 200, "{v}");
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    let error = v["result"]["isError"] == true;
    (error, serde_json::from_str(text).unwrap_or(json!(text)))
}

#[tokio::test]
async fn blocks_change_and_move_files_attach_and_tokens_show_use() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let wid = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, cred) = api(
        &router,
        "POST",
        "mcp-credentials",
        &owner,
        json!({"workspace_id": wid, "name": "Agent", "access": "read_write"}),
    )
    .await;
    let token = cred["token"].as_str().unwrap().to_string();
    assert!(cred["last_used_at"].is_null(), "{cred}");

    let (error, made) = call(
        &router,
        &wid,
        &token,
        "create_document",
        json!({"folder_id": folder, "title": "Plan", "markdown": "One\n\nTwo\n\nThree"}),
    )
    .await;
    assert!(!error, "{made}");
    let nid = made["document_id"].as_str().unwrap().to_string();
    let (_, n) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    let ids: Vec<String> = n["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap().to_string())
        .collect();
    let texts = |n: &Value| -> Vec<String> {
        n["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["markdown"].as_str().unwrap().to_string())
            .collect()
    };

    // Change a block, then move blocks around.
    let (error, changed) = call(
        &router,
        &wid,
        &token,
        "update_block",
        json!({"document_id": nid, "block_id": ids[1], "markdown": "Two, revised"}),
    )
    .await;
    assert!(!error, "{changed}");
    let (error, moved) = call(
        &router,
        &wid,
        &token,
        "move_block",
        json!({"document_id": nid, "block_id": ids[2]}),
    )
    .await;
    assert!(!error, "{moved}");
    let (error, _) = call(
        &router,
        &wid,
        &token,
        "move_block",
        json!({"document_id": nid, "block_id": ids[0], "after_block_id": ids[1]}),
    )
    .await;
    assert!(!error);
    let (_, n) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    assert_eq!(texts(&n), vec!["Three", "Two, revised", "One"]);
    let (_, history) = api(
        &router,
        "GET",
        &format!("notes/{nid}/history"),
        &owner,
        json!({}),
    )
    .await;
    assert!(
        history.as_array().is_some_and(|h| h.len() >= 3),
        "earlier versions are kept: {history}"
    );
    for (args, says) in [
        (
            json!({"document_id": nid, "block_id": "nope", "markdown": "x"}),
            "No such block",
        ),
        (
            json!({"document_id": nid, "block_id": ids[0], "after_block_id": ids[0]}),
            "after itself",
        ),
    ] {
        let tool = if args.get("markdown").is_some() {
            "update_block"
        } else {
            "move_block"
        };
        let (error, said) = call(&router, &wid, &token, tool, args).await;
        assert!(error && said.to_string().contains(says), "{said}");
    }

    // A file, attached and shown in a block.
    use base64::Engine;
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    let (error, attached) = call(&router, &wid, &token, "attach_file", json!({"document_id": nid, "filename": "chart.png", "content_base64": base64::engine::general_purpose::STANDARD.encode(&png), "after_block_id": ids[2]})).await;
    assert!(!error, "{attached}");
    assert_eq!(attached["media_type"], "image/png");
    let (_, n) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    let shown = texts(&n);
    assert_eq!(
        shown[1],
        format!(
            "![chart.png](/api/v1/attachments/{})",
            attached["attachment_id"].as_str().unwrap()
        ),
        "{shown:?}"
    );
    let (error, said) = call(
        &router,
        &wid,
        &token,
        "attach_file",
        json!({"document_id": nid, "filename": "x.bin", "content_base64": "not base64!"}),
    )
    .await;
    assert!(error, "{said}");

    // A read-only credential lists none of them.
    let (_, reader) = api(
        &router,
        "POST",
        "mcp-credentials",
        &owner,
        json!({"workspace_id": wid, "name": "Reader"}),
    )
    .await;
    let (error, _) = call(
        &router,
        &wid,
        reader["token"].as_str().unwrap(),
        "move_block",
        json!({"document_id": nid, "block_id": ids[0]}),
    )
    .await;
    assert!(error);

    // The list says when each credential was last used.
    let (_, list) = api(&router, "GET", "mcp-credentials", &owner, json!({})).await;
    let agent = list
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Agent")
        .unwrap();
    assert!(agent["last_used_at"].as_u64().is_some(), "{agent}");

    // Personal tokens: read only ones read, and are refused anything else.
    let (_, ro) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name": "Backup", "access": "read_only"}),
    )
    .await;
    assert_eq!(ro["access"], "read_only");
    let ro = ro["token"].as_str().unwrap().to_string();
    let (status, _) = api(&router, "GET", &format!("notes/{nid}"), &ro, json!({})).await;
    assert_eq!(status, 200);
    let (status, _) = api(&router, "POST", "search", &ro, json!({"query": "revised"})).await;
    assert_eq!(status, 200);
    let (status, said) = api(
        &router,
        "POST",
        "notes",
        &ro,
        json!({"folder_id": folder, "title": "No"}),
    )
    .await;
    assert_eq!(status, 403, "{said}");
    let (status, _) = api(&router, "DELETE", &format!("notes/{nid}"), &ro, json!({})).await;
    assert_eq!(status, 403);
    let (_, rw) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name": "Script"}),
    )
    .await;
    assert_eq!(rw["access"], "read_write");
    let (status, _) = api(
        &router,
        "POST",
        "notes",
        rw["token"].as_str().unwrap(),
        json!({"folder_id": folder, "title": "Yes"}),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name": "Odd", "access": "everything"}),
    )
    .await;
    assert_eq!(status, 400);
    let (_, tokens) = api(&router, "GET", "api-tokens", &owner, json!({})).await;
    let backup = tokens
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "Backup")
        .unwrap();
    assert!(backup["last_used_at"].as_u64().is_some(), "{backup}");
    let unused = tokens
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["name"] != "Odd");
    assert!(unused);
}
