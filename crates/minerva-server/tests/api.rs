use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/api/v1/{path}"))
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn login(app: &Router, email: &str) -> String {
    let (status, v) = request(
        app,
        "POST",
        "login",
        "",
        json!({"email":email,"password":"test-password-1234"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["token"].as_str().unwrap().into()
}
#[tokio::test]
async fn permissions_conflicts_history_and_revocation() {
    let app = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    assert_eq!(
        request(&app, "GET", "bootstrap", "", json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    let root = login(&app, "admin@test.local").await;
    let admin = common::member(&app, &root, "owner@test.local").await;
    let (_, boot) = request(&app, "GET", "bootstrap", &admin, json!({})).await;
    let folder = common::team_folder(&boot)["id"].as_str().unwrap();
    let nid = common::welcome(&boot)["id"].as_str().unwrap();
    let (_, user) = request(
        &app,
        "POST",
        "users",
        &root,
        json!({"name":"Reader","email":"reader@test.local","password":"test-password-1234"}),
    )
    .await;
    let reader = login(&app, "reader@test.local").await;
    let (_, boot) = request(&app, "GET", "bootstrap", &reader, json!({})).await;
    // Nothing of anyone else's: only the guide in their own workspace.
    let notes = boot["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["title"], "Getting Started");
    // Only their own personal workspace.
    let workspaces = boot["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 1);
    assert_eq!(
        (
            workspaces[0]["name"].as_str(),
            workspaces[0]["personal"].as_bool()
        ),
        (Some("My Workspace"), Some(true))
    );
    assert_eq!(
        request(&app, "GET", &format!("notes/{nid}"), &reader, json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "workspaces",
            &reader,
            json!({"name":"Reader's own"})
        )
        .await
        .0,
        StatusCode::OK,
        "every member creates workspaces"
    );
    assert_eq!(
        request(&app, "GET", "admin", &reader, json!({})).await.0,
        StatusCode::FORBIDDEN
    );
    let (_, hidden_search) = request(
        &app,
        "POST",
        "search",
        &reader,
        json!({"query":"little space for big ideas"}),
    )
    .await;
    assert_eq!(hidden_search.as_array().unwrap().len(), 0);
    let grant = json!({"folder_id":folder,"user_id":user["id"],"role":"viewer"});
    assert_eq!(
        request(&app, "PUT", "permissions", &admin, grant.clone())
            .await
            .0,
        StatusCode::OK
    );
    let (_, visible_search) = request(
        &app,
        "POST",
        "search",
        &reader,
        json!({"query":"Rust server"}),
    )
    .await;
    assert_eq!(visible_search.as_array().unwrap().len(), 1);
    let (_, mut note) = request(&app, "GET", &format!("notes/{nid}"), &reader, json!({})).await;
    assert_eq!(
        request(&app, "PUT", &format!("notes/{nid}"), &reader, note.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut grant = grant;
    grant["role"] = json!("editor");
    request(&app, "PUT", "permissions", &admin, grant.clone()).await;
    note["title"] = json!("Updated by editor");
    assert_eq!(
        request(&app, "PUT", &format!("notes/{nid}"), &reader, note.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "PUT", &format!("notes/{nid}"), &reader, note.clone())
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (_, history) = request(
        &app,
        "GET",
        &format!("notes/{nid}/history"),
        &reader,
        json!({}),
    )
    .await;
    // The welcome note's first save, then the editor's change.
    assert_eq!(history.as_array().unwrap().len(), 2);
    grant["role"] = json!("none");
    request(&app, "PUT", "permissions", &admin, grant).await;
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("notes/{nid}/history"),
            &reader,
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    request(
        &app,
        "PATCH",
        &format!("users/{}", user["id"].as_str().unwrap()),
        &root,
        json!({"disabled":true}),
    )
    .await;
    assert_eq!(
        request(&app, "GET", "bootstrap", &reader, json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn persistence_sanitization_and_csrf() {
    let path = std::env::temp_dir().join(format!("minerva-test-{}.db", uuid::Uuid::new_v4()));
    let p = path.to_str().unwrap();
    let server = app(Store::open(p, "admin@test.local", "test-password-1234").unwrap());
    let root = login(&server, "admin@test.local").await;
    let token = common::member(&server, &root, "owner@test.local").await;
    let (_, workspace) = request(
        &server,
        "POST",
        "workspaces",
        &token,
        json!({"name":"Persist me"}),
    )
    .await;
    let (_,render)=request(&server,"POST","render",&token,json!({"markdown":"# Safe\n<script>alert(1)</script><img src=x onerror=alert(1)>[bad](javascript:alert(1))"})).await;
    let html = render["html"].as_str().unwrap();
    assert!(html.contains("<h1>Safe</h1>"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("onerror"));
    assert!(!html.contains("href=\"javascript:"));
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/workspaces")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from("{\"name\":\"CSRF\"}"))
        .unwrap();
    assert_eq!(
        server.clone().oneshot(req).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let (_, unsafe_link) = request(
        &server,
        "POST",
        "render",
        &token,
        json!({"markdown":"[bad](javascript:alert%281%29)"}),
    )
    .await;
    assert!(!unsafe_link["html"].as_str().unwrap().contains("href="));
    drop(server);
    let reopened = app(Store::open(p, "ignored", "ignored").unwrap());
    let (_, boot) = request(&reopened, "GET", "bootstrap", &token, json!({})).await;
    assert!(boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == workspace["id"]));
    request(&reopened, "POST", "logout", &token, json!({})).await;
    assert_eq!(
        request(&reopened, "GET", "bootstrap", &token, json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn collaborative_text_merges_idempotently_and_survives_restart() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use yrs::{
        updates::decoder::Decode, Doc, GetString, ReadTxn, StateVector, Text, Transact, Update,
    };
    let path = std::env::temp_dir().join(format!("minerva-crdt-{}.db", uuid::Uuid::new_v4()));
    let server = app(Store::open(
        path.to_str().unwrap(),
        "admin@test.local",
        "test-password-1234",
    )
    .unwrap());
    let root = login(&server, "admin@test.local").await;
    let token = common::member(&server, &root, "owner@test.local").await;
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    let nid = common::welcome(&boot)["id"].as_str().unwrap();
    let route = format!("notes/{nid}/sync");
    let (_, initial) = request(&server, "POST", &route, &token, json!({"client_id":"a"})).await;
    let bid = initial["note"]["blocks"][0]["id"].as_str().unwrap();
    let alice = Doc::with_client_id(111);
    let bob = Doc::with_client_id(222);
    for doc in [&alice, &bob] {
        doc.transact_mut()
            .apply_update(
                Update::decode_v1(
                    &STANDARD
                        .decode(initial["update"].as_str().unwrap())
                        .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let a = alice.get_or_insert_text(format!("block:{bid}"));
    let b = bob.get_or_insert_text(format!("block:{bid}"));
    a.insert(&mut alice.transact_mut(), 0, "Alice ");
    b.insert(&mut bob.transact_mut(), 0, "Bob ");
    let update_a = STANDARD.encode(
        alice
            .transact()
            .encode_state_as_update_v1(&StateVector::default()),
    );
    let update_b = STANDARD.encode(
        bob.transact()
            .encode_state_as_update_v1(&StateVector::default()),
    );
    assert_eq!(
        request(
            &server,
            "POST",
            &route,
            &token,
            json!({"update":update_a,"client_id":"a"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, merged) = request(
        &server,
        "POST",
        &route,
        &token,
        json!({"update":update_b,"client_id":"b"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    let text = merged["note"]["blocks"][0]["markdown"].as_str().unwrap();
    assert!(text.contains("Alice ") && text.contains("Bob "));
    assert_eq!(merged["peers"].as_array().unwrap().len(), 2);
    for doc in [&alice, &bob] {
        doc.transact_mut()
            .apply_update(
                Update::decode_v1(&STANDARD.decode(merged["update"].as_str().unwrap()).unwrap())
                    .unwrap(),
            )
            .unwrap();
    }
    assert_eq!(
        a.get_string(&alice.transact()),
        b.get_string(&bob.transact())
    );
    let (_, duplicate) = request(&server, "POST", &route, &token, json!({"update":update_a})).await;
    assert_eq!(duplicate["note"]["revision"], merged["note"]["revision"]);
    let (_, rendered) = request(&server, "GET", &format!("notes/{nid}"), &token, json!({})).await;
    assert_eq!(rendered["blocks"][0]["markdown"], text);
    drop(server);
    let reopened = app(Store::open(path.to_str().unwrap(), "unused", "unused").unwrap());
    let (_, persisted) = request(&reopened, "POST", &route, &token, json!({})).await;
    assert_eq!(persisted["note"]["blocks"][0]["markdown"], text);
    assert_eq!(persisted["peers"].as_array().unwrap().len(), 0);
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn typed_records_enforce_schema_versions_and_permissions() {
    let server = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&server, "admin@test.local").await;
    let admin = common::member(&server, &root, "owner@test.local").await;
    let (_, boot) = request(&server, "GET", "bootstrap", &admin, json!({})).await;
    let fid = &common::team_folder(&boot)["id"];
    let columns = json!([
        {"id":"name","name":"Name","kind":"text"},
        {"id":"number","name":"Estimate","kind":"number"},
        {"id":"date","name":"Due","kind":"date"},
        {"id":"done","name":"Done","kind":"checkbox"},
        {"id":"status","name":"Status","kind":"select","options":["Todo","Done"]}
    ]);
    let (status, db) = request(
        &server,
        "POST",
        "databases",
        &admin,
        json!({"name":"Roadmap","folder_id":fid,"columns":columns}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{db}");
    let did = db["id"].as_str().unwrap();
    let route = format!("databases/{did}/rows");
    for cells in [
        json!({"number":"not numeric"}),
        json!({"date":"2025-02-29"}),
        json!({"date":"20-002-001"}),
        json!({"done":"yes"}),
        json!({"status":"Other"}),
        json!({"unknown":"value"}),
    ] {
        assert_eq!(
            request(&server, "POST", &route, &admin, json!({"cells":cells}))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let(_,row)=request(&server,"POST",&route,&admin,json!({"cells":{"name":"Launch","number":4.5,"date":"2024-02-29","done":false,"status":"Todo"}})).await;
    let rr = format!("{route}/{}", row["id"].as_str().unwrap());
    assert_eq!(
        request(
            &server,
            "PATCH",
            &rr,
            &admin,
            json!({"revision":1,"cells":{"done":true}})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &server,
            "PATCH",
            &rr,
            &admin,
            json!({"revision":1,"cells":{"name":"Stale"}})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &server,
            "PATCH",
            &rr,
            &admin,
            json!({"revision":2,"archived":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &server,
            "PATCH",
            &rr,
            &admin,
            json!({"revision":3,"archived":false})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, user) = request(
        &server,
        "POST",
        "users",
        &root,
        json!({"name":"Viewer","email":"viewer@test.local","password":"test-password-1234"}),
    )
    .await;
    let viewer = login(&server, "viewer@test.local").await;
    assert_eq!(
        request(
            &server,
            "GET",
            &format!("databases/{did}"),
            &viewer,
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    request(
        &server,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":fid,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    assert_eq!(
        request(
            &server,
            "GET",
            &format!("databases/{did}"),
            &viewer,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&server, "POST", &route, &viewer, json!({"cells":{}}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let nid = common::welcome(&boot)["id"].as_str().unwrap();
    assert_eq!(
        request(
            &server,
            "POST",
            &format!("notes/{nid}/sync"),
            &viewer,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &server,
            "POST",
            &format!("notes/{nid}/sync"),
            &viewer,
            json!({"update":"AAA="})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
async fn mcp_request(
    server: &Router,
    wid: &str,
    token: &str,
    body: Value,
    origin: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/api/workspaces/{wid}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("authorization", format!("Bearer {token}"));
    if let Some(origin) = origin {
        req = req.header("origin", origin);
    }
    let res = server
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let raw = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        if raw.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&raw).unwrap()
        },
    )
}
#[tokio::test]
async fn mcp_is_scoped_read_only_revocable_and_origin_checked() {
    let server = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&server, "admin@test.local").await;
    let admin = common::member(&server, &root, "owner@test.local").await;
    let (_, boot) = request(&server, "GET", "bootstrap", &admin, json!({})).await;
    let wid = common::team_workspace(&boot)["id"].as_str().unwrap();
    let fid = &common::team_folder(&boot)["id"];
    let (_, second) = request(
        &server,
        "POST",
        "workspaces",
        &admin,
        json!({"name":"Other workspace"}),
    )
    .await;
    let (_, user) = request(
        &server,
        "POST",
        "users",
        &root,
        json!({"name":"AI user","email":"ai@test.local","password":"test-password-1234"}),
    )
    .await;
    request(
        &server,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":fid,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let actor = login(&server, "ai@test.local").await;
    let (_, credential) = request(
        &server,
        "POST",
        "mcp-credentials",
        &actor,
        json!({"workspace_id":wid,"name":"Test MCP","expires_days":1}),
    )
    .await;
    let token = credential["token"].as_str().unwrap();
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}});
    assert_eq!(
        mcp_request(&server, wid, token, init.clone(), None).await.1["result"]["protocolVersion"],
        "2025-06-18"
    );
    assert_eq!(
        mcp_request(&server, wid, &actor, init.clone(), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mcp_request(
            &server,
            second["id"].as_str().unwrap(),
            token,
            init.clone(),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mcp_request(
            &server,
            wid,
            token,
            init.clone(),
            Some("https://evil.example")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        mcp_request(
            &server,
            wid,
            token,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            None
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    let (_, tools) = mcp_request(
        &server,
        wid,
        token,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        None,
    )
    .await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 5);
    assert!(tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["annotations"]["readOnlyHint"] == true));
    let search = json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"doc_search","arguments":{"query":"Rust server"}}});
    let (_, results) = mcp_request(&server, wid, token, search.clone(), None).await;
    assert_eq!(results["result"]["isError"], false);
    let content: Value =
        serde_json::from_str(results["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content["matches"].as_array().unwrap().len(), 1);
    let (_, all) = request(&server, "GET", "bootstrap", &admin, json!({})).await;
    let other_folder = all["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] == second["id"])
        .unwrap();
    let (_, hidden) = request(
        &server,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":other_folder["id"],"title":"Other workspace secret"}),
    )
    .await;
    let (_, hidden_result) = mcp_request(&server,wid,token,json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"read_document","arguments":{"document_id":hidden["id"]}}}),None).await;
    assert_eq!(hidden_result["result"]["isError"], true);
    let (_, listed) = request(&server, "GET", "mcp-credentials", &actor, json!({})).await;
    assert!(listed[0].get("token").is_none());
    assert!(listed[0].get("token_hash").is_none());
    request(
        &server,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":fid,"user_id":user["id"],"role":"none"}),
    )
    .await;
    assert_eq!(
        mcp_request(&server, wid, token, search, None).await.0,
        StatusCode::FORBIDDEN
    );
    request(
        &server,
        "DELETE",
        &format!("mcp-credentials/{}", credential["id"].as_str().unwrap()),
        &actor,
        json!({}),
    )
    .await;
    assert_eq!(
        mcp_request(&server, wid, token, init, None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn attachment_storage_permissions_headers_and_restart() {
    let path =
        std::env::temp_dir().join(format!("minerva-attachments-{}.db", uuid::Uuid::new_v4()));
    let store = Store::open(
        path.to_str().unwrap(),
        "admin@test.local",
        "test-password-1234",
    )
    .unwrap();
    let router = app(store.clone());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let (_, boot) = request(&router, "GET", "bootstrap", &admin, json!({})).await;
    let nid = common::welcome(&boot)["id"].as_str().unwrap();
    let folder = common::team_folder(&boot)["id"].as_str().unwrap();
    let upload_path = format!("/api/v1/notes/{nid}/attachments?filename=..%2F..%2Ftest%0D%0A.png");
    let bytes = b"\x89PNG\r\n\x1a\noriginal bytes";
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&upload_path)
                .header("authorization", format!("Bearer {admin}"))
                .header("x-minerva-client", "1")
                .body(Body::from(bytes.as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let file: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(file["filename"], "test.png");
    assert_eq!(file["media_type"], "image/png");
    assert!(uuid::Uuid::parse_str(file["id"].as_str().unwrap()).is_ok());
    let url = format!("/api/v1/attachments/{}", file["id"].as_str().unwrap());
    let (_, user) = request(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Reader","email":"reader@test.local","password":"test-password-1234"}),
    )
    .await;
    let reader = login(&router, "reader@test.local").await;
    async fn get_file(router: &Router, url: &str, token: &str) -> axum::response::Response {
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(url)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    assert_eq!(
        get_file(&router, &url, "").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get_file(&router, &url, &reader).await.status(),
        StatusCode::FORBIDDEN
    );
    request(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":folder,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let download = get_file(&router, &url, &reader).await;
    assert_eq!(download.status(), StatusCode::OK);
    assert!(download.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment;"));
    assert_eq!(
        download.headers()["content-type"],
        "application/octet-stream"
    );
    assert_eq!(
        download
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        bytes
    );
    let preview = get_file(&router, &format!("{url}/preview"), &reader).await;
    assert_eq!(preview.headers()["content-type"], "image/png");
    assert_eq!(preview.headers()["x-content-type-options"], "nosniff");
    let rejected = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&upload_path)
                .header("authorization", format!("Bearer {reader}"))
                .header("x-minerva-client", "1")
                .body(Body::from("bad"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
    let csrf = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&upload_path)
                .header("authorization", format!("Bearer {admin}"))
                .body(Body::from("bad"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(csrf.status(), StatusCode::BAD_REQUEST);
    let large = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&upload_path)
                .header("authorization", format!("Bearer {admin}"))
                .header("x-minerva-client", "1")
                .body(Body::from(vec![0u8; 10 * 1024 * 1024 + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(large.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let html = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/notes/{nid}/attachments?filename=fake.png"))
                .header("authorization", format!("Bearer {admin}"))
                .header("x-minerva-client", "1")
                .body(Body::from("<script>alert(1)</script>"))
                .unwrap(),
        )
        .await
        .unwrap();
    let html: Value =
        serde_json::from_slice(&html.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(html["media_type"], "application/octet-stream");
    assert_eq!(
        get_file(
            &router,
            &format!(
                "/api/v1/attachments/{}/preview",
                html["id"].as_str().unwrap()
            ),
            &admin
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    request(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":folder,"user_id":user["id"],"role":"none"}),
    )
    .await;
    assert_eq!(
        get_file(&router, &url, &reader).await.status(),
        StatusCode::FORBIDDEN
    );
    drop(router);
    drop(store);
    let reopened = app(Store::open(path.to_str().unwrap(), "", "").unwrap());
    assert_eq!(
        get_file(&reopened, &url, &admin)
            .await
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        bytes
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn fenced_and_inline_code_render_as_safe_text() {
    let router = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&router, "admin@test.local").await;
    let token = common::member(&router, &root, "owner@test.local").await;
    let (_, rendered) = request(
        &router,
        "POST",
        "render",
        &token,
        json!({"markdown":"```rust\nlet x = \"<script>\";\n```\n\nInline `visible`"}),
    )
    .await;
    let html = rendered["html"].as_str().unwrap();
    assert!(html.contains("<pre><code"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("<code>visible</code>"));
}

#[tokio::test]
async fn markdown_images_allow_remote_urls_and_preview_uploaded_files() {
    let router = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&router, "admin@test.local").await;
    let token = common::member(&router, &root, "owner@test.local").await;
    let key = uuid::Uuid::new_v4();
    let path = format!("/api/v1/attachments/{key}");
    let markdown = format!("![Remote](https://example.com/photo.png)\n\n![HTTP](http://example.com/photo.jpg)\n\n![Upload]({path})\n\n![Absolute](http://localhost:3001{path})\n\n[Download]({path})\n\n`![Code]({path})`\n\n![Unsafe](javascript:alert)");
    let (_, rendered) = request(
        &router,
        "POST",
        "render",
        &token,
        json!({"markdown":markdown}),
    )
    .await;
    let html = rendered["html"].as_str().unwrap();
    assert!(html.contains("src=\"https://example.com/photo.png\""));
    assert!(html.contains("src=\"http://example.com/photo.jpg\""));
    assert!(html.contains(&format!("src=\"{path}/preview\"")));
    assert!(html.contains(&format!("src=\"http://localhost:3001{path}/preview\"")));
    assert!(html.contains(&format!("href=\"{path}\"")));
    assert!(html.contains(&format!("<code>![Code]({path})</code>")));
    assert!(!html.contains("javascript:"));
    let page = router
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let policy = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(policy.contains("img-src 'self' data: http: https:;"));
    assert!(policy.contains("script-src 'self' https://www.youtube.com https://s.ytimg.com;"));
    assert!(!policy.contains("script-src 'self' 'unsafe-inline'"));
}

#[tokio::test]
async fn ai_configuration_and_context_are_permission_scoped() {
    let router = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let (_, boot) = request(&router, "GET", "bootstrap", &admin, json!({})).await;
    let folder = common::team_folder(&boot)["id"].as_str().unwrap();
    let workspace = common::team_workspace(&boot)["id"].as_str().unwrap();
    let note = common::welcome(&boot)["id"].as_str().unwrap();
    let (_, hidden) = request(
        &router,
        "POST",
        "folders",
        &admin,
        json!({"workspace_id":workspace,"name":"Secret folder"}),
    )
    .await;
    let (_, hidden_note) = request(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":hidden["id"],"title":"Never send hidden content"}),
    )
    .await;
    let (_, other) = request(
        &router,
        "POST",
        "workspaces",
        &admin,
        json!({"name":"Other workspace"}),
    )
    .await;
    let (_, user) = request(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Member","email":"reader@test.local","password":"test-password-1234"}),
    )
    .await;
    let reader = login(&router, "reader@test.local").await;
    request(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":folder,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let config = json!({"kind":"openai","name":"Team","endpoint":"https://api.openai.com/v1","model":"test-model","api_key":"never-return-this-key"});
    assert_eq!(
        request(&router, "POST", "ai/providers", &reader, config.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, p) = request(&router, "POST", "ai/providers", &root, config).await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert!(p.get("api_key").is_none());
    let (_, listed) = request(&router, "GET", "ai/providers", &reader, json!({})).await;
    assert!(!listed.to_string().contains("never-return"));
    assert_eq!(request(&router,"POST","ai/providers",&root,json!({"kind":"compatible","name":"Blocked","endpoint":"ftp://127.0.0.1:1","model":"test","api_key":"key"})).await.0,StatusCode::BAD_REQUEST);
    for (kind, id) in [("workspace", workspace), ("folder", folder), ("note", note)] {
        let (status, chat) = request(
            &router,
            "POST",
            "ai/chats",
            &reader,
            json!({"scope":{"kind":kind,"id":id}}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{chat}");
        let cid = chat["id"].as_str().unwrap();
        let (status, context) = request(
            &router,
            "GET",
            &format!("ai/chats/{cid}/context"),
            &reader,
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(context["sources"].as_array().unwrap().len(), 1);
        assert_eq!(context["sources"][0]["id"], note);
        assert!(!context.to_string().contains("Never send hidden"));
        assert_eq!(
            request(
                &router,
                "GET",
                &format!("ai/chats/{cid}"),
                &admin,
                json!({})
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    for scope in [
        json!({"kind":"folder","id":hidden["id"]}),
        json!({"kind":"note","id":hidden_note["id"]}),
        json!({"kind":"workspace","id":other["id"]}),
    ] {
        assert_eq!(
            request(&router, "POST", "ai/chats", &reader, json!({"scope":scope}))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    request(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":folder,"user_id":user["id"],"role":"none"}),
    )
    .await;
    let (_, chats) = request(&router, "GET", "ai/chats", &reader, json!({})).await;
    assert!(chats.as_array().unwrap().is_empty());
    assert_eq!(
        request(
            &router,
            "DELETE",
            &format!("ai/providers/{}", p["id"].as_str().unwrap()),
            &reader,
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn only_the_editor_pane_allows_same_origin_framing() {
    let router = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    for (path, expected) in [
        ("/", "frame-ancestors 'none'"),
        ("/pane", "frame-ancestors 'self'"),
    ] {
        let response = router
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let policy = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(policy.contains(expected), "{policy}");
        assert!(policy.contains("https://www.youtube.com"));
        assert!(!policy.contains("frame-ancestors *"));
    }
}
