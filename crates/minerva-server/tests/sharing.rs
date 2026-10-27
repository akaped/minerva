//! Member shares, share links, publishing, and reference-aware attachment cleanup.
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
use yrs::{updates::decoder::Decode, Doc, ReadTxn, StateVector, Text, Transact, Update};
mod common;

const PASSWORD: &str = "test-password-1234";

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}
/// Sends a request with optional bearer token, share cookie, and body.
async fn send(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    cookie: &str,
    body: impl Into<Body>,
) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1");
    if !token.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if !cookie.is_empty() {
        request = request.header("cookie", cookie);
    }
    let response = router
        .clone()
        .oneshot(request.body(body.into()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    Reply {
        status,
        headers,
        body,
    }
}
async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> Reply {
    send(
        router,
        method,
        &format!("/api/v1/{path}"),
        token,
        "",
        body.to_string(),
    )
    .await
}
async fn login(router: &Router, email: &str) -> String {
    let reply = api(
        router,
        "POST",
        "login",
        "",
        json!({"email":email,"password":PASSWORD}),
    )
    .await;
    reply.json()["token"].as_str().unwrap().to_string()
}
async fn setup(router: &Router) -> (String, String, Value, String) {
    let root = login(router, "admin@test.local").await;
    let admin = common::member(router, &root, "owner@test.local").await;
    let boot = api(router, "GET", "bootstrap", &admin, json!({}))
        .await
        .json();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let note = api(
        router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Roadmap"}),
    )
    .await
    .json();
    let mut save = note.clone();
    save["blocks"] = json!([{"id":"b1","markdown":"## Plan\nShip **Aurora**."},{"id":"b2","markdown":"```rust\nfn main() {}\n```"}]);
    let saved = api(
        router,
        "PUT",
        &format!("notes/{}", note["id"].as_str().unwrap()),
        &admin,
        save,
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text());
    api(
        router,
        "POST",
        "users",
        &root,
        json!({"name":"Robin Reader","email":"reader@test.local","password":PASSWORD}),
    )
    .await;
    let reader = login(router, "reader@test.local").await;
    (admin, reader, saved.json(), folder)
}
async fn upload(router: &Router, uri: &str, token: &str, cookie: &str, bytes: &[u8]) -> Reply {
    send(router, "POST", uri, token, cookie, bytes.to_vec()).await
}

#[tokio::test]
async fn member_shares_grant_one_note_and_are_revocable() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (admin, reader, note, _) = setup(&router).await;
    let nid = note["id"].as_str().unwrap();
    assert_eq!(
        api(&router, "GET", &format!("notes/{nid}"), &reader, json!({}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let unknown = api(
        &router,
        "POST",
        &format!("notes/{nid}/shares"),
        &admin,
        json!({"kind":"member","email":"nobody@test.local"}),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    let share = api(
        &router,
        "POST",
        &format!("notes/{nid}/shares"),
        &admin,
        json!({"kind":"member","email":"READER@test.local"}),
    )
    .await;
    assert_eq!(share.status, StatusCode::OK, "{}", share.text());
    assert_eq!(
        share.json()["share"]["role"],
        "viewer",
        "read-only is the default"
    );

    let view = api(&router, "GET", &format!("notes/{nid}"), &reader, json!({})).await;
    assert_eq!(view.status, StatusCode::OK);
    assert_eq!(view.json()["role"], "viewer");
    let boot = api(&router, "GET", "bootstrap", &reader, json!({}))
        .await
        .json();
    assert!(
        boot["notes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["title"] == "Getting Started"),
        "no folder access is implied: only the guide in their own workspace"
    );
    assert_eq!(boot["shared_notes"][0]["id"], nid);
    let found = api(
        &router,
        "POST",
        "search",
        &reader,
        json!({"query":"aurora"}),
    )
    .await
    .json();
    assert_eq!(found[0]["id"], nid);
    let mut edit = view.json();
    edit["title"] = json!("Changed by reader");
    assert_eq!(
        api(
            &router,
            "PUT",
            &format!("notes/{nid}"),
            &reader,
            edit.clone()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            &router,
            "POST",
            &format!("notes/{nid}/sync"),
            &reader,
            json!({"client_id":"r"})
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        api(
            &router,
            "POST",
            &format!("notes/{nid}/sync"),
            &reader,
            json!({"update":"AAA="})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{nid}/attachments"),
            &reader,
            json!({})
        )
        .await
        .status,
        StatusCode::OK
    );
    // Recipients cannot re-share.
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{nid}/shares"),
            &reader,
            json!({})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            &router,
            "POST",
            &format!("notes/{nid}/shares"),
            &reader,
            json!({"kind":"link"})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // Upgrading replaces the share rather than duplicating it.
    api(
        &router,
        "POST",
        &format!("notes/{nid}/shares"),
        &admin,
        json!({"kind":"member","email":"reader@test.local","role":"editor"}),
    )
    .await;
    let listed = api(
        &router,
        "GET",
        &format!("notes/{nid}/shares"),
        &admin,
        json!({}),
    )
    .await
    .json();
    assert_eq!(listed["shares"].as_array().unwrap().len(), 1);
    assert_eq!(listed["shares"][0]["user"]["email"], "reader@test.local");
    let edit = api(&router, "PUT", &format!("notes/{nid}"), &reader, edit).await;
    assert_eq!(edit.status, StatusCode::OK, "{}", edit.text());
    assert_eq!(edit.json()["role"], "editor");

    let sharing = api(&router, "GET", "sharing", &admin, json!({}))
        .await
        .json();
    assert_eq!(sharing["managed"][0]["sharing"]["members"], 1);

    let sid = listed["shares"][0]["id"].as_str().unwrap();
    assert_eq!(
        api(
            &router,
            "DELETE",
            &format!("notes/{nid}/shares/{sid}"),
            &admin,
            json!({})
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        api(&router, "GET", &format!("notes/{nid}"), &reader, json!({}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn share_links_enforce_mode_on_note_sync_and_attachment_routes() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (admin, _, note, _) = setup(&router).await;
    let nid = note["id"].as_str().unwrap().to_string();
    let file = upload(
        &router,
        &format!("/api/v1/notes/{nid}/attachments?filename=plan.png"),
        &admin,
        "",
        b"\x89PNG\r\n\x1a\nimage",
    )
    .await
    .json();
    let other = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":note["folder_id"],"title":"Other"}),
    )
    .await
    .json();
    let foreign = upload(
        &router,
        &format!(
            "/api/v1/notes/{}/attachments?filename=x.png",
            other["id"].as_str().unwrap()
        ),
        &admin,
        "",
        b"\x89PNG\r\n\x1a\nother",
    )
    .await
    .json();

    let created = api(
        &router,
        "POST",
        &format!("notes/{nid}/shares"),
        &admin,
        json!({"kind":"link"}),
    )
    .await
    .json();
    let token = created["token"].as_str().unwrap().to_string();
    assert_eq!(created["share"]["role"], "viewer");
    let listed = api(
        &router,
        "GET",
        &format!("notes/{nid}/shares"),
        &admin,
        json!({}),
    )
    .await
    .text();
    assert!(!listed.contains(&token), "raw tokens are never listed");

    assert_eq!(
        api(
            &router,
            "POST",
            "shared/session",
            "",
            json!({"token":"wrong"})
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(&router, "GET", "/api/v1/shared/note", "", "", Body::empty())
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let session = api(
        &router,
        "POST",
        "shared/session",
        "",
        json!({"token":token}),
    )
    .await;
    assert_eq!(session.status, StatusCode::OK);
    let set_cookie = session.headers["set-cookie"].to_str().unwrap().to_string();
    assert!(
        set_cookie.contains("HttpOnly")
            && set_cookie.contains("SameSite=Strict")
            && set_cookie.contains("Path=/api/v1/shared")
    );
    let cookie = format!("minerva_share={token}");

    let view = send(
        &router,
        "GET",
        "/api/v1/shared/note",
        "",
        &cookie,
        Body::empty(),
    )
    .await;
    assert_eq!(view.status, StatusCode::OK);
    let view = view.json();
    assert_eq!(view["title"], "Roadmap");
    assert_eq!(view["role"], "viewer");
    assert_eq!(
        view["folder_id"], "shared",
        "folder identity is not disclosed"
    );
    assert!(!view
        .to_string()
        .contains(note["folder_id"].as_str().unwrap()));
    assert!(
        view["rendered"][1].as_str().unwrap().contains("hl-"),
        "code is highlighted for guests too"
    );
    // The share cookie is not a member session.
    assert_eq!(
        send(
            &router,
            "GET",
            "/api/v1/bootstrap",
            "",
            &cookie,
            Body::empty()
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    // Viewer link: read sync allowed, writes refused.
    let read = send(
        &router,
        "POST",
        "/api/v1/shared/sync",
        "",
        &cookie,
        json!({"client_id":"g"}).to_string(),
    )
    .await;
    assert_eq!(read.status, StatusCode::OK);
    let read = read.json();
    assert_eq!(read["can_edit"], false);
    let doc = Doc::with_client_id(7);
    doc.transact_mut()
        .apply_update(
            Update::decode_v1(&STANDARD.decode(read["update"].as_str().unwrap()).unwrap()).unwrap(),
        )
        .unwrap();
    let block = doc.get_or_insert_text("block:b1");
    block.insert(&mut doc.transact_mut(), 0, "Guest edit. ");
    let update = STANDARD.encode(
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default()),
    );
    let denied = send(
        &router,
        "POST",
        "/api/v1/shared/sync",
        "",
        &cookie,
        json!({"update":update}).to_string(),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    assert_eq!(
        upload(
            &router,
            "/api/v1/shared/attachments?filename=a.txt",
            "",
            &cookie,
            b"data"
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // Attachments: only this note's files.
    let mine = send(
        &router,
        "GET",
        &format!(
            "/api/v1/shared/attachments/{}/preview",
            file["id"].as_str().unwrap()
        ),
        "",
        &cookie,
        Body::empty(),
    )
    .await;
    assert_eq!(mine.status, StatusCode::OK);
    assert_eq!(mine.body, b"\x89PNG\r\n\x1a\nimage");
    let theirs = send(
        &router,
        "GET",
        &format!(
            "/api/v1/shared/attachments/{}",
            foreign["id"].as_str().unwrap()
        ),
        "",
        &cookie,
        Body::empty(),
    )
    .await;
    assert_eq!(theirs.status, StatusCode::FORBIDDEN);
    let rendered = send(
        &router,
        "POST",
        "/api/v1/shared/render",
        "",
        &cookie,
        json!({"markdown":format!("![x](/api/v1/attachments/{})", file["id"].as_str().unwrap())})
            .to_string(),
    )
    .await
    .json();
    assert!(rendered["html"]
        .as_str()
        .unwrap()
        .contains("/api/v1/shared/attachments/"));

    // Editor link: CRDT updates and uploads are accepted.
    let editor = api(
        &router,
        "POST",
        &format!("notes/{nid}/shares"),
        &admin,
        json!({"kind":"link","role":"editor"}),
    )
    .await
    .json();
    let editor_cookie = format!("minerva_share={}", editor["token"].as_str().unwrap());
    let merged = send(
        &router,
        "POST",
        "/api/v1/shared/sync",
        "",
        &editor_cookie,
        json!({"update":update,"client_id":"g2"}).to_string(),
    )
    .await;
    assert_eq!(merged.status, StatusCode::OK, "{}", merged.text());
    let merged = merged.json();
    assert!(merged["note"]["blocks"][0]["markdown"]
        .as_str()
        .unwrap()
        .starts_with("Guest edit. "));
    assert_eq!(merged["can_edit"], true);
    let member_view = api(&router, "GET", &format!("notes/{nid}"), &admin, json!({}))
        .await
        .json();
    assert!(member_view["blocks"][0]["markdown"]
        .as_str()
        .unwrap()
        .starts_with("Guest edit. "));
    // Members see guests as anonymous peers; guests see members anonymized.
    let peers = api(
        &router,
        "POST",
        &format!("notes/{nid}/sync"),
        &admin,
        json!({"client_id":"m"}),
    )
    .await
    .json()["peers"]
        .clone();
    assert!(peers
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "Guest"));
    let guest_peers = send(
        &router,
        "POST",
        "/api/v1/shared/sync",
        "",
        &editor_cookie,
        json!({"client_id":"g2"}).to_string(),
    )
    .await
    .json()["peers"]
        .clone();
    assert!(guest_peers
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "Member"));
    assert!(!guest_peers.to_string().contains("Administrator"));
    let uploaded = upload(
        &router,
        "/api/v1/shared/attachments?filename=guest.txt",
        "",
        &editor_cookie,
        b"guest file",
    )
    .await;
    assert_eq!(uploaded.status, StatusCode::CREATED);

    // Revocation and trash end access immediately.
    let sid = created["share"]["id"].as_str().unwrap();
    api(
        &router,
        "DELETE",
        &format!("notes/{nid}/shares/{sid}"),
        &admin,
        json!({}),
    )
    .await;
    assert_eq!(
        send(
            &router,
            "GET",
            "/api/v1/shared/note",
            "",
            &cookie,
            Body::empty()
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &router,
            "GET",
            "/api/v1/shared/note",
            "",
            &editor_cookie,
            Body::empty()
        )
        .await
        .status,
        StatusCode::OK
    );
    let mut trash = api(&router, "GET", &format!("notes/{nid}"), &admin, json!({}))
        .await
        .json();
    trash["trashed"] = json!(true);
    api(&router, "PUT", &format!("notes/{nid}"), &admin, trash).await;
    assert_eq!(
        send(
            &router,
            "GET",
            "/api/v1/shared/note",
            "",
            &editor_cookie,
            Body::empty()
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(
            &router,
            "POST",
            &format!("notes/{nid}/shares"),
            &admin,
            json!({"kind":"link"})
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn published_pages_are_clean_pinned_or_live_and_expose_only_referenced_files() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (admin, reader, note, folder) = setup(&router).await;
    let nid = note["id"].as_str().unwrap().to_string();
    let file = upload(
        &router,
        &format!("/api/v1/notes/{nid}/attachments?filename=chart.png"),
        &admin,
        "",
        b"\x89PNG\r\n\x1a\nchart",
    )
    .await
    .json();
    let hidden = upload(
        &router,
        &format!("/api/v1/notes/{nid}/attachments?filename=draft.png"),
        &admin,
        "",
        b"\x89PNG\r\n\x1a\ndraft",
    )
    .await
    .json();
    let fid = file["id"].as_str().unwrap();
    let mut save = api(&router, "GET", &format!("notes/{nid}"), &admin, json!({}))
        .await
        .json();
    save["blocks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"img","markdown":format!("![Chart](/api/v1/attachments/{fid})")}));
    let save = api(&router, "PUT", &format!("notes/{nid}"), &admin, save)
        .await
        .json();

    for slug in ["Bad Slug", "-x", "", "feed.xml"] {
        assert_eq!(
            api(
                &router,
                "PUT",
                &format!("notes/{nid}/publication"),
                &admin,
                json!({"slug":slug})
            )
            .await
            .status,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        api(
            &router,
            "PUT",
            &format!("notes/{nid}/publication"),
            &reader,
            json!({"slug":"roadmap"})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let published = api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        &admin,
        json!({"slug":"roadmap","description":"Our plan for <2027>"}),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.text());
    assert_eq!(published.json()["mode"], "pinned", "pinned is the default");

    let page = send(&router, "GET", "/shared/roadmap", "", "", Body::empty()).await;
    assert_eq!(page.status, StatusCode::OK);
    // Pages used to live under /p/: old links land on the same page.
    let old = send(&router, "GET", "/p/roadmap?x=1", "", "", Body::empty()).await;
    assert_eq!(old.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(old.headers["location"], "/shared/roadmap?x=1");
    let html = page.text();
    assert!(html.contains("<h1>Roadmap</h1>") && html.contains("Aurora"));
    assert!(html.contains("Our plan for &lt;2027&gt;"));
    assert!(html.contains(&format!("/shared/roadmap/files/{fid}/preview")));
    for private in [
        "Getting started",
        "My workspace",
        "admin@test.local",
        "Administrator",
        folder.as_str(),
        "contenteditable",
        "app.js",
    ] {
        assert!(!html.contains(private), "published page leaks {private}");
    }
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/shared/roadmap/files/{fid}/preview"),
            "",
            "",
            Body::empty()
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/shared/roadmap/files/{}", hidden["id"].as_str().unwrap()),
            "",
            "",
            Body::empty()
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
        "unreferenced files stay private"
    );

    // Pinned: later edits do not appear until the published version is updated.
    let mut edit = save.clone();
    edit["blocks"][0]["markdown"] = json!("Ship **Borealis** instead.");
    let edit = api(&router, "PUT", &format!("notes/{nid}"), &admin, edit)
        .await
        .json();
    assert!(
        send(&router, "GET", "/shared/roadmap", "", "", Body::empty())
            .await
            .text()
            .contains("Aurora")
    );
    api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        &admin,
        json!({"slug":"roadmap","refresh":true}),
    )
    .await;
    assert!(
        send(&router, "GET", "/shared/roadmap", "", "", Body::empty())
            .await
            .text()
            .contains("Borealis")
    );
    // Live: follows edits.
    api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        &admin,
        json!({"slug":"roadmap","mode":"live"}),
    )
    .await;
    let mut edit = edit.clone();
    edit["blocks"][0]["markdown"] = json!("Live wording.");
    api(&router, "PUT", &format!("notes/{nid}"), &admin, edit).await;
    assert!(
        send(&router, "GET", "/shared/roadmap", "", "", Body::empty())
            .await
            .text()
            .contains("Live wording.")
    );

    let other = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Second"}),
    )
    .await
    .json();
    assert_eq!(
        api(
            &router,
            "PUT",
            &format!("notes/{}/publication", other["id"].as_str().unwrap()),
            &admin,
            json!({"slug":"roadmap"})
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    api(
        &router,
        "PUT",
        &format!("notes/{}/publication", other["id"].as_str().unwrap()),
        &admin,
        json!({"slug":"second"}),
    )
    .await;
    let index = send(&router, "GET", "/shared/", "", "", Body::empty())
        .await
        .text();
    assert!(
        index.contains("href=\"/shared/roadmap\"") && index.contains("href=\"/shared/second\"")
    );
    let feed = send(&router, "GET", "/shared/feed.xml", "", "", Body::empty()).await;
    assert!(feed.headers["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/atom+xml"));
    assert!(feed.text().contains("<entry><title>Roadmap</title>"));
    let boot = api(&router, "GET", "bootstrap", &admin, json!({}))
        .await
        .json();
    let summary = boot["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == nid.as_str())
        .unwrap()["sharing"]
        .clone();
    assert_eq!(summary["published"], "shared/roadmap");

    // Preview requires a member who manages sharing.
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/preview/{nid}"),
            "",
            "",
            Body::empty()
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/preview/{nid}"),
            &reader,
            "",
            Body::empty()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let preview = send(
        &router,
        "GET",
        &format!("/preview/{nid}"),
        &admin,
        "",
        Body::empty(),
    )
    .await;
    assert!(preview.text().contains("not public until you publish"));

    // Unpublishing and trashing take pages down.
    api(
        &router,
        "DELETE",
        &format!("notes/{nid}/publication"),
        &admin,
        json!({}),
    )
    .await;
    assert_eq!(
        send(&router, "GET", "/shared/roadmap", "", "", Body::empty())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let mut trash = api(
        &router,
        "GET",
        &format!("notes/{}", other["id"].as_str().unwrap()),
        &admin,
        json!({}),
    )
    .await
    .json();
    trash["trashed"] = json!(true);
    api(
        &router,
        "PUT",
        &format!("notes/{}", other["id"].as_str().unwrap()),
        &admin,
        trash,
    )
    .await;
    assert_eq!(
        send(&router, "GET", "/shared/second", "", "", Body::empty())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn storage_cleanup_only_deletes_files_no_content_references() {
    let path = std::env::temp_dir().join(format!("minerva-storage-{}.db", uuid::Uuid::new_v4()));
    let router = app(Store::open(path.to_str().unwrap(), "admin@test.local", PASSWORD).unwrap());
    let (admin, reader, note, _) = setup(&router).await;
    let root = login(&router, "admin@test.local").await;
    let nid = note["id"].as_str().unwrap().to_string();
    let mut ids = vec![];
    for name in ["current", "history", "published", "orphan", "recent"] {
        let reply = upload(
            &router,
            &format!("/api/v1/notes/{nid}/attachments?filename={name}.bin"),
            &admin,
            "",
            name.as_bytes(),
        )
        .await;
        ids.push(reply.json()["id"].as_str().unwrap().to_string());
    }
    let (current, in_history, in_publication, orphan, recent) =
        (&ids[0], &ids[1], &ids[2], &ids[3], &ids[4]);
    let link = |id: &str| format!("[file](/api/v1/attachments/{id})");
    // Reference `history` and `published`, publish (pinned), then remove them from the note.
    let mut save = api(&router, "GET", &format!("notes/{nid}"), &admin, json!({}))
        .await
        .json();
    save["blocks"] = json!([{"id":"a","markdown":link(current)},{"id":"b","markdown":link(in_history)},{"id":"c","markdown":link(in_publication)}]);
    let save = api(&router, "PUT", &format!("notes/{nid}"), &admin, save)
        .await
        .json();
    api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        &admin,
        json!({"slug":"files"}),
    )
    .await;
    let mut save = save.clone();
    save["blocks"] =
        json!([{"id":"a","markdown":link(current)},{"id":"c","markdown":link(in_publication)}]);
    let save = api(&router, "PUT", &format!("notes/{nid}"), &admin, save)
        .await
        .json();
    let mut save = save.clone();
    save["blocks"] = json!([{"id":"a","markdown":link(current)}]);
    api(&router, "PUT", &format!("notes/{nid}"), &admin, save).await;

    // Age every upload except `recent` past the grace period.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE attachments SET created_at = created_at - 172800 WHERE id != ?1",
        [recent],
    )
    .unwrap();

    assert_eq!(
        api(&router, "GET", "admin/storage", &reader, json!({}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let report = api(&router, "GET", "admin/storage", &root, json!({}))
        .await
        .json();
    assert_eq!(report["files"], 5);
    let orphans: Vec<&str> = report["orphans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap())
        .collect();
    assert_eq!(orphans, vec![orphan.as_str()], "{report}");
    assert_eq!(report["recent_unreferenced"], 1);
    assert_eq!(common::welcome(&report)["note_id"], nid);

    assert_eq!(
        api(
            &router,
            "POST",
            "admin/storage/cleanup",
            &reader,
            json!({"ids":[orphan]})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let cleaned = api(
        &router,
        "POST",
        "admin/storage/cleanup",
        &root,
        json!({"ids":[orphan, current, in_history, in_publication, recent]}),
    )
    .await
    .json();
    assert_eq!(cleaned["deleted"], 1);
    assert_eq!(
        cleaned["kept"], 4,
        "referenced and recent files are never deleted"
    );
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/api/v1/attachments/{orphan}"),
            &admin,
            "",
            Body::empty()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    for kept in [current, in_history, in_publication, recent] {
        assert_eq!(
            send(
                &router,
                "GET",
                &format!("/api/v1/attachments/{kept}"),
                &admin,
                "",
                Body::empty()
            )
            .await
            .status,
            StatusCode::OK
        );
    }
    drop(db);
    std::fs::remove_file(&path).ok();
}
