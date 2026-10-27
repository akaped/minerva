//! Public sites: per-workspace paths such as `/blog` and `/wiki` for published pages.
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

const PASSWORD: &str = "test-password-1234";

async fn send(
    router: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Body,
) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(body)
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
async fn get(router: &Router, uri: &str) -> (StatusCode, String) {
    send(router, "GET", uri, "", Body::empty()).await
}
async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let (status, text) = send(
        router,
        method,
        &format!("/api/v1/{path}"),
        token,
        Body::from(body.to_string()),
    )
    .await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}
async fn login(router: &Router, email: &str) -> String {
    let (_, v) = api(
        router,
        "POST",
        "login",
        "",
        json!({"email":email,"password":PASSWORD}),
    )
    .await;
    v["token"].as_str().unwrap().to_string()
}
async fn user(router: &Router, admin: &str, name: &str) -> (String, String) {
    let email = format!("{name}@test.local");
    let (_, v) = api(
        router,
        "POST",
        "users",
        admin,
        json!({"name":name,"email":email,"password":PASSWORD,"can_create_workspace":true}),
    )
    .await;
    (
        v["id"].as_str().unwrap().to_string(),
        login(router, &email).await,
    )
}
/// Creates a workspace and returns its ID and General folder.
async fn workspace(router: &Router, token: &str, name: &str) -> (String, String) {
    let (_, w) = api(router, "POST", "workspaces", token, json!({"name":name})).await;
    let wid = w["id"].as_str().unwrap().to_string();
    let (_, boot) = api(router, "GET", "bootstrap", token, json!({})).await;
    let folder = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] == wid)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    (wid, folder)
}
async fn note(router: &Router, token: &str, folder: &str, title: &str, markdown: &str) -> String {
    let (_, mut n) = api(
        router,
        "POST",
        "notes",
        token,
        json!({"folder_id":folder,"title":title}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    n["blocks"] = json!([{"id":"b1","markdown":markdown}]);
    let (status, _) = api(router, "PUT", &format!("notes/{nid}"), token, n).await;
    assert_eq!(status, StatusCode::OK);
    nid
}
async fn publish(router: &Router, token: &str, nid: &str, body: Value) -> (StatusCode, Value) {
    api(
        router,
        "PUT",
        &format!("notes/{nid}/publication"),
        token,
        body,
    )
    .await
}

#[tokio::test]
async fn sites_group_pages_under_their_own_paths() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let (_, alice) = user(&router, &root, "alice").await;
    let (_, bob) = user(&router, &root, "bob").await;
    let (carol_id, carol) = user(&router, &root, "carol").await;
    let (ws, folder) = workspace(&router, &alice, "Alpha").await;
    let (bob_ws, bob_folder) = workspace(&router, &bob, "Beta").await;

    // Owners create sites; paths are unique and cannot shadow server routes.
    let (status, blog) = api(
        &router,
        "POST",
        "sites",
        &alice,
        json!({"workspace_id":ws,"path":"blog","title":"Alice writes"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{blog}");
    assert_eq!(blog["layout"], "blog");
    let (_, wiki) = api(&router, "POST", "sites", &alice, json!({"workspace_id":ws,"path":"wiki","title":"Dragon wiki","layout":"wiki","description":"Everything about dragons"})).await;
    for path in ["api", "p", "export", "Blog", "a/b"] {
        let (status, _) = api(
            &router,
            "POST",
            "sites",
            &alice,
            json!({"workspace_id":ws,"path":path}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
    }
    let (status, _) = api(
        &router,
        "POST",
        "sites",
        &bob,
        json!({"workspace_id":bob_ws,"path":"blog"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "paths are first come, first served"
    );
    let (status, _) = api(
        &router,
        "POST",
        "sites",
        &bob,
        json!({"workspace_id":ws,"path":"bobs"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let blog_id = blog["id"].as_str().unwrap();
    let wiki_id = wiki["id"].as_str().unwrap();
    for method in ["PATCH", "DELETE"] {
        let (status, _) = api(
            &router,
            method,
            &format!("sites/{blog_id}"),
            &bob,
            json!({"title":"Mine"}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}");
    }
    let (_, boot) = api(&router, "GET", "bootstrap", &bob, json!({})).await;
    assert_eq!(
        boot["sites"],
        json!([]),
        "other workspaces' sites stay private"
    );
    let (_, boot) = api(&router, "GET", "bootstrap", &alice, json!({})).await;
    assert_eq!(boot["sites"].as_array().unwrap().len(), 2);

    // Folder editors publish into their workspace's sites, not into others'.
    api(
        &router,
        "PUT",
        "permissions",
        &alice,
        json!({"folder_id":folder,"user_id":carol_id,"role":"editor"}),
    )
    .await;
    let (status, _) = api(
        &router,
        "POST",
        "sites",
        &carol,
        json!({"workspace_id":ws,"path":"carol"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "only owners manage sites");
    let hello = note(&router, &alice, &folder, "Hello world", "First post").await;
    let (status, page) = publish(
        &router,
        &carol,
        &hello,
        json!({"slug":"hello","site_id":blog_id,"mode":"live"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["path"], "blog/hello");
    let bobs = note(&router, &bob, &bob_folder, "Intruder", "x").await;
    let (status, _) = publish(
        &router,
        &bob,
        &bobs,
        json!({"slug":"intruder","site_id":blog_id}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A wiki page linking to the blog post, with an image; plus a /shared/ page.
    let dragon = note(&router, &alice, &folder, "How to train your dragon", "").await;
    let (_, file) = send(
        &router,
        "POST",
        &format!("/api/v1/notes/{dragon}/attachments?filename=d.png"),
        &alice,
        Body::from(b"\x89PNG\r\n\x1a\n0000".to_vec()),
    )
    .await;
    let fid = serde_json::from_str::<Value>(&file).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, mut n) = api(
        &router,
        "GET",
        &format!("notes/{dragon}"),
        &alice,
        json!({}),
    )
    .await;
    n["blocks"] = json!([{"id":"b1","markdown":format!("See [the intro](#note/{hello}) and [a secret](#note/{bobs}).\n\n![](/api/v1/attachments/{fid})")}]);
    api(&router, "PUT", &format!("notes/{dragon}"), &alice, n).await;
    let (status, _) = publish(
        &router,
        &alice,
        &dragon,
        json!({"slug":"how-to-train-your-dragon","site_id":wiki_id,"mode":"live"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let apples = note(&router, &alice, &folder, "Apples", "Fruit").await;
    publish(
        &router,
        &alice,
        &apples,
        json!({"slug":"apples","site_id":wiki_id,"mode":"live"}),
    )
    .await;
    let plain = note(&router, &alice, &folder, "Plain", "Classic").await;
    let (_, p) = publish(
        &router,
        &alice,
        &plain,
        json!({"slug":"hello","mode":"live"}),
    )
    .await;
    assert_eq!(
        p["path"], "shared/hello",
        "the same slug may exist in another site"
    );

    let (status, index) = get(&router, "/blog/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(index.contains("Alice writes") && index.contains("href=\"/blog/hello\""));
    assert!(!index.contains("dragon") && !index.contains("Plain"));
    assert!(index.contains("href=\"/blog/feed.xml\""));
    assert_eq!(get(&router, "/blog").await.0, StatusCode::OK);
    let (status, post) = get(&router, "/blog/hello").await;
    assert!(
        status == StatusCode::OK && post.contains("First post") && post.contains("← All posts")
    );
    let (status, feed) = get(&router, "/blog/feed.xml").await;
    assert!(status == StatusCode::OK && feed.contains("/blog/hello</id>"));

    let (_, index) = get(&router, "/wiki/").await;
    assert!(index.contains("Everything about dragons"));
    let (a, d) = (
        index.find("Apples").unwrap(),
        index.find("How to train").unwrap(),
    );
    assert!(a < d, "wiki pages are listed alphabetically");
    assert!(!index.contains("<time"), "wiki indexes have no dates");
    assert_eq!(
        get(&router, "/wiki/feed.xml").await.0,
        StatusCode::NOT_FOUND
    );
    let (status, page) = get(&router, "/wiki/how-to-train-your-dragon").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("href=\"/blog/hello\""),
        "links to published notes resolve"
    );
    assert!(
        !page.contains(&bobs),
        "links to unpublished notes are dropped"
    );
    assert!(page.contains("Last updated") && page.contains("← All pages"));
    assert!(page.contains(&format!(
        "/wiki/how-to-train-your-dragon/files/{fid}/preview"
    )));
    assert_eq!(
        get(
            &router,
            &format!("/wiki/how-to-train-your-dragon/files/{fid}/preview")
        )
        .await
        .0,
        StatusCode::OK
    );

    let (_, index) = get(&router, "/shared/").await;
    assert!(index.contains("Plain") && !index.contains("Hello world"));
    assert_eq!(
        get(&router, "/shared/how-to-train-your-dragon").await.0,
        StatusCode::NOT_FOUND
    );
    for missing in ["/nowhere/", "/nowhere/hello", "/wiki/hello", "/api"] {
        assert_eq!(
            get(&router, missing).await.0,
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }
    assert_eq!(
        get(&router, "/app.js").await.0,
        StatusCode::OK,
        "server files still win"
    );

    // Updating without a site keeps the page in place; renaming moves the site.
    let (_, kept) = publish(
        &router,
        &alice,
        &hello,
        json!({"slug":"hello","description":"Updated","mode":"live"}),
    )
    .await;
    assert_eq!(kept["path"], "blog/hello");
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("sites/{blog_id}"),
        &alice,
        json!({"path":"journal-log"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(get(&router, "/blog/hello").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&router, "/journal-log/hello").await.0, StatusCode::OK);

    // Sites with pages cannot be removed until the pages move or are unpublished.
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("sites/{blog_id}"),
        &alice,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, moved) = publish(
        &router,
        &alice,
        &hello,
        json!({"slug":"hello-again","site_id":"","mode":"live"}),
    )
    .await;
    assert_eq!(moved["path"], "shared/hello-again");
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("sites/{blog_id}"),
        &alice,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(get(&router, "/journal-log/").await.0, StatusCode::NOT_FOUND);
}
