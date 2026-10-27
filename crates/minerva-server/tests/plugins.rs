//! Frontend plugins: app themes and public website themes, built in or from a folder.
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
    body: Value,
) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
async fn get(router: &Router, uri: &str) -> (StatusCode, String) {
    send(router, "GET", uri, "", Value::Null).await
}
async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let (status, text) = send(router, method, &format!("/api/v1/{path}"), token, body).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}
fn ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn themes_load_from_builtins_and_the_plugin_folder() {
    // One test owns the process-wide plugin folder setting.
    let dir = std::env::temp_dir().join(format!("minerva-plugins-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::fs::create_dir_all(dir.join("public-notes-themes")).unwrap();
    std::env::set_var("MINERVA_PLUGINS", &dir);
    std::env::set_var("MINERVA_PUBLIC_THEME", "midnight");
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());

    let (status, index) = get(&router, "/plugins/index.json").await;
    assert_eq!(status, StatusCode::OK);
    let index: Value = serde_json::from_str(&index).unwrap();
    assert_eq!(
        ids(&index["themes"]),
        ["classic-light", "colorful", "terminal"]
    );
    assert_eq!(index["themes"][2]["name"], "Command line");
    assert_eq!(index["themes"][2]["base"], "dark");
    assert_eq!(
        ids(&index["public_themes"]),
        ["midnight", "paper", "sunrise", "terminal"]
    );
    let (status, css) = get(&router, "/plugins/themes/terminal.css").await;
    assert!(status == StatusCode::OK && css.contains("[data-variant=\"terminal\"]"));
    for missing in [
        "/plugins/themes/nope.css",
        "/plugins/themes/..%2Fx.css",
        "/plugins/other/a.css",
        "/plugins/themes/terminal",
    ] {
        assert_eq!(
            get(&router, missing).await.0,
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }

    // Files in the plugin folder add themes and override built-ins, live.
    std::fs::write(
        dir.join("themes/ocean.css"),
        "/*\n * name: Ocean\n * base: dark\n */\n[data-variant=\"ocean\"]{--accent:#0af}",
    )
    .unwrap();
    std::fs::write(
        dir.join("public-notes-themes/paper.css"),
        "/*\n * name: My paper\n * scheme: light\n */\n:root{--page:#ffe}",
    )
    .unwrap();
    std::fs::write(dir.join("themes/Bad Name.css"), "x").unwrap();
    let (_, index) = get(&router, "/plugins/index.json").await;
    let index: Value = serde_json::from_str(&index).unwrap();
    assert_eq!(
        ids(&index["themes"]),
        ["classic-light", "colorful", "terminal", "ocean"]
    );
    assert_eq!(
        index["public_themes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "paper")
            .unwrap()["name"],
        "My paper"
    );
    assert!(get(&router, "/plugins/themes/ocean.css")
        .await
        .1
        .contains("#0af"));

    // Sites pick a public theme; pages link it and pin its color scheme.
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let owner = common::member(&router, &root, "owner@test.local").await;
    let admin = owner.as_str();
    let (_, boot) = api(&router, "GET", "bootstrap", admin, json!({})).await;
    let wid = common::team_workspace(&boot)["id"].as_str().unwrap();
    let folder = common::team_folder(&boot)["id"].as_str().unwrap();
    let (status, _) = api(
        &router,
        "POST",
        "sites",
        admin,
        json!({"workspace_id":wid,"path":"x","theme":"missing"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, site) = api(
        &router,
        "POST",
        "sites",
        admin,
        json!({"workspace_id":wid,"path":"journal-site","theme":"paper"}),
    )
    .await;
    assert_eq!(
        (status, site["theme"].as_str()),
        (StatusCode::OK, Some("paper"))
    );
    let (_, n) = api(
        &router,
        "POST",
        "notes",
        admin,
        json!({"folder_id":folder,"title":"Hi"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap();
    api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        admin,
        json!({"slug":"hi","site_id":site["id"],"mode":"live"}),
    )
    .await;
    let (_, page) = get(&router, "/journal-site/hi").await;
    assert!(page.contains("<html lang=\"en\" data-scheme=\"light\">"));
    assert!(page.contains("href=\"/plugins/public-notes-themes/paper.css\""));
    let (_, index_page) = get(&router, "/journal-site/").await;
    assert!(index_page.contains("public-notes-themes/paper.css"));

    // Pages without a site use MINERVA_PUBLIC_THEME.
    let (_, p) = get(&router, "/shared/").await;
    assert!(p.contains("public-notes-themes/midnight.css") && p.contains("data-scheme=\"dark\""));

    // Clearing the theme, or removing its file, falls back to the default look.
    std::fs::remove_file(dir.join("public-notes-themes/paper.css")).unwrap();
    let (_, page) = get(&router, "/journal-site/hi").await;
    assert!(
        page.contains("public-notes-themes/paper.css"),
        "the built-in paper theme remains"
    );
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("sites/{}", site["id"].as_str().unwrap()),
        admin,
        json!({"theme":""}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, page) = get(&router, "/journal-site/hi").await;
    assert!(!page.contains("public-notes-themes") && page.contains("<html lang=\"en\">"));
    std::env::set_var("MINERVA_PUBLIC_THEME", "gone");
    let (_, p) = get(&router, "/shared/").await;
    assert!(!p.contains("public-notes-themes"));
    std::fs::remove_dir_all(&dir).unwrap();
}
