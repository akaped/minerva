//! Links to blocks and notes shown inside other notes: what an embed shows,
//! to whom, and how exports and published pages treat it.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn send(router: &Router, method: &str, uri: &str, token: &str, body: Value) -> (u16, String) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    let (status, text) = send(router, method, &format!("/api/v1/{path}"), token, body).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

#[tokio::test]
async fn embeds_show_blocks_and_notes_to_those_who_can_open_them() {
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
    let reader = common::account(&router, &root, "reader@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let wid = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let open = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, private) = api(
        &router,
        "POST",
        "folders",
        &owner,
        json!({"name":"Private","workspace_id":wid}),
    )
    .await;
    let create = |folder: String, title: &'static str| {
        let router = router.clone();
        let owner = owner.clone();
        async move {
            api(
                &router,
                "POST",
                "notes",
                &owner,
                json!({"folder_id":folder,"title":title}),
            )
            .await
            .1["id"]
                .as_str()
                .unwrap()
                .to_string()
        }
    };
    let set_blocks = |id: String, blocks: Vec<(&'static str, String)>| {
        let router = router.clone();
        let owner = owner.clone();
        async move {
            let path = format!("notes/{id}");
            let (_, mut note) = api(&router, "GET", &path, &owner, json!({})).await;
            note["blocks"] = json!(blocks
                .iter()
                .map(|(id, md)| json!({"id": id, "markdown": md, "x": 0, "y": 0}))
                .collect::<Vec<_>>());
            let (status, saved) = api(&router, "PUT", &path, &owner, note).await;
            assert_eq!(status, 200, "{saved}");
            saved
        }
    };
    let source = create(open.clone(), "Decisions").await;
    set_blocks(
        source.clone(),
        vec![
            ("top", "- We ship **Friday**".into()),
            ("child", "  - after the review".into()),
            ("other", "- Unrelated".into()),
        ],
    )
    .await;
    let secret = create(private["id"].as_str().unwrap().into(), "Secret").await;
    set_blocks(secret.clone(), vec![("s", "Hidden plan".into())]).await;
    let host = create(open.clone(), "Weekly").await;
    set_blocks(
        host.clone(),
        vec![
            ("h1", format!("![Ship date](#note/{source}#top)")),
            (
                "h2",
                format!("See [the decision](#note/{source}#top) and ![secret](#note/{secret})."),
            ),
            ("h3", format!("`![code](#note/{source})`")),
        ],
    )
    .await;
    let (_, reader_boot) = api(&router, "GET", "bootstrap", &reader, json!({})).await;
    api(
        &router,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id":open,"user_id":reader_boot["user"]["id"],"role":"viewer"}),
    )
    .await;

    // A block brings the blocks nested under it; a whole note brings them all.
    let (status, shown) = api(
        &router,
        "POST",
        "embeds",
        &reader,
        json!({"items":[
            {"note": source, "block": "top"},
            {"note": source},
            {"note": source, "block": "gone"},
            {"note": secret},
            {"note": "0b8f5f9e-0000-4000-8000-000000000000"},
        ]}),
    )
    .await;
    assert_eq!(status, 200, "{shown}");
    let block = &shown[0];
    assert_eq!(block["state"], "ok");
    assert_eq!(block["title"], "Decisions");
    let html: Vec<&str> = block["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["html"].as_str().unwrap())
        .collect();
    assert_eq!(html.len(), 2, "{block}");
    assert!(html[0].contains("<strong>Friday</strong>") && html[1].contains("after the review"));
    assert!(
        !html[1].contains("md-indent-2"),
        "nesting starts at the embed"
    );
    assert_eq!(shown[1]["blocks"].as_array().unwrap().len(), 3);
    assert_eq!(shown[2]["state"], "no-block");
    // A note the reader cannot open looks the same as one that does not exist.
    assert_eq!(
        shown[3],
        json!({"note": secret, "block": null, "state": "unavailable"})
    );
    assert_eq!(shown[4]["state"], "unavailable");

    // Nothing is sent again while the note is unchanged.
    let revision = block["revision"].clone();
    let (_, again) = api(
        &router,
        "POST",
        "embeds",
        &reader,
        json!({"items":[{"note": source, "block": "top", "revision": revision}]}),
    )
    .await;
    assert_eq!(again[0]["unchanged"], true);
    assert!(again[0]["blocks"].is_null());
    set_blocks(
        source.clone(),
        vec![
            ("top", "- We ship **Monday**".into()),
            ("child", "  - after the review".into()),
        ],
    )
    .await;
    let (_, changed) = api(
        &router,
        "POST",
        "embeds",
        &reader,
        json!({"items":[{"note": source, "block": "top", "revision": revision}]}),
    )
    .await;
    assert!(changed[0]["blocks"][0]["html"]
        .as_str()
        .unwrap()
        .contains("Monday"));

    let (status, _) = api(
        &router,
        "POST",
        "embeds",
        &reader,
        json!({"items":[{"note":"x"}]}),
    )
    .await;
    assert_eq!(status, 400);
    let many: Vec<Value> = (0..51).map(|_| json!({"note": source})).collect();
    let (status, _) = api(&router, "POST", "embeds", &reader, json!({ "items": many })).await;
    assert_eq!(status, 400);

    // The host note renders slots, and both notes count as linked.
    let (_, viewed) = api(&router, "GET", &format!("notes/{host}"), &owner, json!({})).await;
    assert!(viewed["rendered"][0]
        .as_str()
        .unwrap()
        .contains(&format!("note-embed ne-note-{source} ne-block-top")));
    let (_, target) = api(
        &router,
        "GET",
        &format!("notes/{source}"),
        &reader,
        json!({}),
    )
    .await;
    assert_eq!(target["backlinks"][0]["id"], host);

    // Exports bring the content in, for what the exporter can open.
    let (status, md) = send(
        &router,
        "GET",
        &format!("/export/{host}?format=md&front_matter=0"),
        &reader,
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{md}");
    assert!(
        md.contains("- We ship **Monday**\n\n  - after the review"),
        "{md}"
    );
    assert!(
        md.contains("[[Decisions]]"),
        "a block link names its note: {md}"
    );
    assert!(
        md.contains("*Embedded note (not available to you)*"),
        "{md}"
    );
    assert!(!md.contains("Hidden plan"));
    assert_eq!(
        md.matches("after the review").count(),
        1,
        "code is not expanded: {md}"
    );

    // Published pages say what is embedded without showing private notes.
    api(
        &router,
        "PUT",
        &format!("notes/{host}/publication"),
        &owner,
        json!({"slug":"weekly","mode":"live"}),
    )
    .await;
    let (_, page) = send(&router, "GET", "/shared/weekly", "", Value::Null).await;
    assert!(page.contains("Embedded note"), "{page}");
    assert!(
        !page.contains("Friday") && !page.contains("Monday"),
        "{page}"
    );
    assert!(!page.contains(&format!("ne-note-{source}")), "{page}");
}
