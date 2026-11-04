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
async fn write_tools_need_write_access_and_edit_rights() {
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
    let viewer = common::account(&router, &root, "viewer@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let wid = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, viewer_boot) = api(&router, "GET", "bootstrap", &viewer, json!({})).await;
    api(
        &router,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id":folder,"user_id":viewer_boot["user"]["id"],"role":"viewer"}),
    )
    .await;
    let credential = |token: String, access: &'static str| {
        let router = router.clone();
        let wid = wid.clone();
        async move {
            let (status, c) = api(
                &router,
                "POST",
                "mcp-credentials",
                &token,
                json!({"workspace_id":wid,"name":"Assistant","expires_days":7,"access":access}),
            )
            .await;
            assert_eq!(status, 200, "{c}");
            assert_eq!(c["access"], access);
            c["token"].as_str().unwrap().to_string()
        }
    };
    let (status, _) = api(
        &router,
        "POST",
        "mcp-credentials",
        &owner,
        json!({"workspace_id":wid,"name":"Bad","expires_days":7,"access":"admin"}),
    )
    .await;
    assert_eq!(status, 400);
    let reader = credential(owner.clone(), "read_only").await;
    let writer = credential(owner.clone(), "read_write").await;
    let viewer_writer = credential(viewer.clone(), "read_write").await;

    // Only write credentials list the write tools.
    let tools = |token: String| {
        let router = router.clone();
        let wid = wid.clone();
        async move {
            let (_, v) = send(
                &router,
                "POST",
                &format!("/api/workspaces/{wid}/mcp"),
                &token,
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            )
            .await;
            v["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(tools(reader.clone()).await.len(), 5);
    let all = tools(writer.clone()).await;
    assert!(all.contains(&"create_document".to_string()) && all.len() == 12);

    // A read-only credential cannot write, nor can a viewer's write credential.
    let create = json!({"folder_id":folder,"title":"Meeting notes","markdown":"# Agenda\n\nBudget review\n\n- Item one\n- Item two"});
    let (error, message) = call(&router, &wid, &reader, "create_document", create.clone()).await;
    assert!(error && message.as_str().unwrap().contains("read only"));
    let (error, _) = call(
        &router,
        &wid,
        &viewer_writer,
        "create_document",
        create.clone(),
    )
    .await;
    assert!(error);

    // Creating a note splits its Markdown into blocks.
    let (error, created) = call(&router, &wid, &writer, "create_document", create).await;
    assert!(!error, "{created}");
    let nid = created["document_id"].as_str().unwrap().to_string();
    let (_, note) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    assert_eq!(note["title"], "Meeting notes");
    assert_eq!(note["blocks"].as_array().unwrap().len(), 4, "{note}");
    assert_eq!(note["blocks"][0]["markdown"], "# Agenda");

    // Appending keeps the old version in history and can go after a block.
    let first = note["blocks"][0]["id"].as_str().unwrap().to_string();
    let (error, appended) = call(
        &router,
        &wid,
        &writer,
        "append_blocks",
        json!({"document_id":nid,"markdown":"Decided: ship it"}),
    )
    .await;
    assert!(!error, "{appended}");
    assert_eq!(appended["revision"], 2);
    call(
        &router,
        &wid,
        &writer,
        "append_blocks",
        json!({"document_id":nid,"markdown":"Attendees: all","after_block_id":first}),
    )
    .await;
    let (_, note) = api(&router, "GET", &format!("notes/{nid}"), &owner, json!({})).await;
    let texts: Vec<&str> = note["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["markdown"].as_str().unwrap())
        .collect();
    assert_eq!(texts[1], "Attendees: all");
    assert_eq!(texts.last().unwrap(), &"Decided: ship it");
    let (_, history) = api(
        &router,
        "GET",
        &format!("notes/{nid}/history"),
        &owner,
        json!({}),
    )
    .await;
    assert!(!history.as_array().unwrap().is_empty(), "{history}");
    let (error, _) = call(
        &router,
        &wid,
        &viewer_writer,
        "append_blocks",
        json!({"document_id":nid,"markdown":"Not allowed"}),
    )
    .await;
    assert!(error);

    // Records: by property name or ID, checked against the schema.
    let (_, tasks) = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id":folder,"name":"Tasks"}),
    )
    .await;
    let did = tasks["id"].as_str().unwrap().to_string();
    let mut columns = tasks["columns"].as_array().unwrap().clone();
    columns.push(json!({"id":"done","name":"Done","kind":"checkbox"}));
    api(
        &router,
        "PATCH",
        &format!("databases/{did}"),
        &owner,
        json!({"revision":tasks["revision"],"columns":columns}),
    )
    .await;
    let name = columns[0]["name"].as_str().unwrap();
    let (error, added) = call(
        &router,
        &wid,
        &writer,
        "add_record",
        json!({"database_id":did,"cells":{name:"Write docs","done":false}}),
    )
    .await;
    assert!(!error, "{added}");
    let rid = added["record"]["id"].as_str().unwrap().to_string();
    let (error, _) = call(
        &router,
        &wid,
        &writer,
        "add_record",
        json!({"database_id":did,"cells":{"Done":"yes"}}),
    )
    .await;
    assert!(error, "a checkbox takes a boolean");
    let (error, _) = call(
        &router,
        &wid,
        &writer,
        "add_record",
        json!({"database_id":did,"cells":{"Missing":"x"}}),
    )
    .await;
    assert!(error);
    let (error, updated) = call(
        &router,
        &wid,
        &writer,
        "update_record",
        json!({"database_id":did,"record_id":rid,"cells":{"Done":true}}),
    )
    .await;
    assert!(!error, "{updated}");
    assert_eq!(updated["record"]["cells"]["done"], true);
    assert_eq!(updated["record"]["revision"], 2);
    let (_, db) = api(
        &router,
        "GET",
        &format!("databases/{did}"),
        &owner,
        json!({}),
    )
    .await;
    let row = &db["rows"][0];
    assert_eq!(row["cells"]["done"], true);
    assert_eq!(
        row["cells"][columns[0]["id"].as_str().unwrap()],
        "Write docs"
    );
    let (error, _) = call(
        &router,
        &wid,
        &viewer_writer,
        "add_record",
        json!({"database_id":did,"cells":{}}),
    )
    .await;
    assert!(error);
}
