//! Note covers, tags, inline tags, tag search, backlinks, and their CRDT and
//! published-page behaviour.
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
use yrs::{updates::decoder::Decode, Any, Doc, Map, ReadTxn, StateVector, Transact, Update};
mod common;

const PASSWORD: &str = "test-password-1234";

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value, String) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1");
    if !token.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::Null),
        text,
    )
}
async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let (status, value, _) = call(router, method, &format!("/api/v1/{path}"), token, body).await;
    (status, value)
}
async fn login(router: &Router, email: &str) -> String {
    api(
        router,
        "POST",
        "login",
        "",
        json!({"email":email,"password":PASSWORD}),
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn covers_tags_inline_tags_search_and_backlinks() {
    let path = std::env::temp_dir().join(format!("minerva-notes-{}.db", uuid::Uuid::new_v4()));
    let router = app(Store::open(path.to_str().unwrap(), "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let boot = api(&router, "GET", "bootstrap", &admin, json!({})).await.1;
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let target = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Target"}),
    )
    .await
    .1;
    let tid = target["id"].as_str().unwrap().to_string();
    assert_eq!(target["cover"], "");
    assert_eq!(target["tags"], json!([]));

    let mut save = target.clone();
    save["blocks"] = json!([{"id":"t1","markdown":"Planning for #Project/Alpha and #ideas"}]);
    save["tags"] = json!(["Work", "#Ideas", "work"]);
    save["cover"] = json!("gradient:3");
    let (status, saved) = api(
        &router,
        "PUT",
        &format!("notes/{tid}"),
        &admin,
        save.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["tags"],
        json!(["work", "ideas"]),
        "tags are normalized and deduplicated"
    );
    assert_eq!(saved["cover"], "gradient:3");
    assert_eq!(saved["inline_tags"], json!(["project/alpha", "ideas"]));

    let mut bad = saved.clone();
    for (field, value) in [
        ("cover", json!("javascript:alert(1)")),
        ("cover", json!("http://example.com/a.png")),
        ("cover", json!("gradient:9")),
        ("cover", json!("https://example.com/a\".png")),
        ("tags", json!(["two words"])),
        ("tags", json!(["#123"])),
        (
            "tags",
            json!((0..21).map(|i| format!("t{i}")).collect::<Vec<_>>()),
        ),
    ] {
        bad[field] = value.clone();
        let (status, _) = api(&router, "PUT", &format!("notes/{tid}"), &admin, bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field} {value}");
        bad = saved.clone();
    }

    // Bootstrap and tag search include note tags and inline tags.
    let boot = api(&router, "GET", "bootstrap", &admin, json!({})).await.1;
    let meta = boot["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == tid.as_str())
        .unwrap()
        .clone();
    assert_eq!(meta["tags"], json!(["work", "ideas"]));
    assert_eq!(meta["inline_tags"], json!(["project/alpha", "ideas"]));
    for query in ["#work", "#proj", "#IDEAS"] {
        let found = api(&router, "POST", "search", &admin, json!({"query":query}))
            .await
            .1;
        assert!(
            found
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n["id"] == tid.as_str()),
            "{query}"
        );
    }
    assert!(api(
        &router,
        "POST",
        "search",
        &admin,
        json!({"query":"#missing"})
    )
    .await
    .1
    .as_array()
    .unwrap()
    .is_empty());

    // Backlinks: only from notes the reader may open.
    let (_, hidden_folder) = api(
        &router,
        "POST",
        "folders",
        &admin,
        json!({"workspace_id":workspace,"name":"Private"}),
    )
    .await;
    let mut sources = vec![];
    for (folder_id, title) in [
        (folder.as_str(), "Visible source"),
        (hidden_folder["id"].as_str().unwrap(), "Hidden source"),
    ] {
        let mut note = api(
            &router,
            "POST",
            "notes",
            &admin,
            json!({"folder_id":folder_id,"title":title}),
        )
        .await
        .1;
        note["blocks"] = json!([{"id":"l","markdown":format!("See [Target](#note/{tid})")}]);
        let note = api(
            &router,
            "PUT",
            &format!("notes/{}", note["id"].as_str().unwrap()),
            &admin,
            note,
        )
        .await
        .1;
        sources.push(note["id"].as_str().unwrap().to_string());
    }
    let view = api(&router, "GET", &format!("notes/{tid}"), &admin, json!({}))
        .await
        .1;
    assert_eq!(view["backlinks"].as_array().unwrap().len(), 2);
    let (_, user) = api(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Reader","email":"reader@test.local","password":PASSWORD}),
    )
    .await;
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":folder,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let reader = login(&router, "reader@test.local").await;
    let view = api(&router, "GET", &format!("notes/{tid}"), &reader, json!({}))
        .await
        .1;
    let backlinks = view["backlinks"].as_array().unwrap();
    assert_eq!(backlinks.len(), 1);
    assert_eq!(backlinks[0]["id"], sources[0].as_str());
    assert_eq!(backlinks[0]["title"], "Visible source");

    // Cover and tags travel through the CRDT and are validated there too.
    let route = format!("notes/{tid}/sync");
    let initial = api(&router, "POST", &route, &admin, json!({"client_id":"c"}))
        .await
        .1;
    let doc = Doc::with_client_id(42);
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
    let meta = doc.get_or_insert_map("meta");
    meta.insert(&mut doc.transact_mut(), "cover", "gradient:7");
    meta.insert(
        &mut doc.transact_mut(),
        "tags",
        Any::Array(vec![Any::from("Live"), Any::from("ideas")].into()),
    );
    let update = STANDARD.encode(
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default()),
    );
    let (status, merged) = api(&router, "POST", &route, &admin, json!({"update":update})).await;
    assert_eq!(status, StatusCode::OK, "{merged}");
    assert_eq!(merged["note"]["cover"], "gradient:7");
    assert_eq!(merged["note"]["tags"], json!(["live", "ideas"]));
    meta.insert(&mut doc.transact_mut(), "cover", "javascript:x");
    let update = STANDARD.encode(
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default()),
    );
    assert_eq!(
        api(&router, "POST", &route, &admin, json!({"update":update}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    // An uploaded cover appears on the published page and is kept by cleanup.
    let upload = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/notes/{tid}/attachments?filename=cover.png"
                ))
                .header("authorization", format!("Bearer {admin}"))
                .header("x-minerva-client", "1")
                .body(Body::from(b"\x89PNG\r\n\x1a\ncover".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    let file: Value =
        serde_json::from_slice(&upload.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let fid = file["id"].as_str().unwrap().to_string();
    let mut current = api(&router, "GET", &format!("notes/{tid}"), &admin, json!({}))
        .await
        .1;
    current["cover"] = json!(format!("/api/v1/attachments/{fid}"));
    assert_eq!(
        api(&router, "PUT", &format!("notes/{tid}"), &admin, current)
            .await
            .0,
        StatusCode::OK
    );
    api(
        &router,
        "PUT",
        &format!("notes/{tid}/publication"),
        &admin,
        json!({"slug":"target"}),
    )
    .await;
    let (_, _, html) = call(&router, "GET", "/shared/target", "", json!({})).await;
    assert!(
        html.contains(&format!("<img src=\"/shared/target/files/{fid}/preview\"")),
        "{html}"
    );
    assert!(html.contains("<span class=\"tag\" data-tag=\"project/alpha\">#Project/Alpha</span>"));
    assert_eq!(
        call(
            &router,
            "GET",
            &format!("/shared/target/files/{fid}/preview"),
            "",
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE attachments SET created_at = created_at - 172800",
        [],
    )
    .unwrap();
    let report = api(&router, "GET", "admin/storage", &root, json!({}))
        .await
        .1;
    assert!(
        report["orphans"].as_array().unwrap().is_empty(),
        "cover images are references: {report}"
    );
    drop(db);
    std::fs::remove_file(&path).ok();
}
