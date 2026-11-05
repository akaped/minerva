//! The installable app: its manifest, icons, service worker and quick-capture
//! page are served to anyone (they hold no notes), with the right types.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::Value;
use tower::ServiceExt;
mod common;

async fn get(router: &Router, path: &str) -> (u16, String, Vec<u8>, String) {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default()
    };
    let (kind, cache) = (header("content-type"), header("cache-control"));
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, kind, bytes.to_vec(), cache)
}

#[tokio::test]
async fn manifest_icons_worker_and_capture_page() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());

    let (status, kind, body, _) = get(&router, "/manifest.webmanifest").await;
    assert_eq!(status, 200);
    assert_eq!(kind, "application/manifest+json");
    let manifest: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(manifest["display"], "standalone");
    // Text, links and files shared from other apps.
    assert_eq!(manifest["share_target"]["action"], "/capture/share");
    assert_eq!(manifest["share_target"]["method"], "POST");
    assert_eq!(
        manifest["share_target"]["params"]["files"][0]["name"],
        "files"
    );
    // Every icon it names is served as a picture.
    let icons = manifest["icons"].as_array().unwrap();
    assert!(icons.iter().any(|i| i["purpose"] == "maskable"));
    for icon in icons {
        let (status, kind, body, _) = get(&router, icon["src"].as_str().unwrap()).await;
        assert_eq!((status, kind.as_str()), (200, "image/png"), "{icon}");
        assert!(body.starts_with(b"\x89PNG"));
    }

    // The worker is named after this build and is checked for updates.
    let (status, kind, body, cache) = get(&router, "/sw.js").await;
    assert_eq!(status, 200);
    assert!(kind.starts_with("text/javascript"));
    assert_eq!(cache, "no-cache");
    let worker = String::from_utf8(body).unwrap();
    assert!(!worker.contains("__VERSION__"));
    let version = worker
        .split("const VERSION = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    assert_eq!(version.len(), 16, "{version}");
    // Everything the worker keeps for opening offline exists.
    for path in [
        "/",
        "/pane",
        "/capture",
        "/capture.js",
        "/app.js",
        "/style.css",
        "/favicon.png",
    ] {
        assert!(worker.contains(&format!("\"{path}\"")), "{path}");
        assert_eq!(get(&router, path).await.0, 200, "{path}");
    }

    let (status, kind, body, _) = get(&router, "/capture").await;
    assert_eq!(status, 200);
    assert!(kind.starts_with("text/html"));
    assert!(String::from_utf8(body).unwrap().contains("/capture.js"));
    let (_, _, index, _) = get(&router, "/").await;
    assert!(String::from_utf8(index)
        .unwrap()
        .contains("rel=\"manifest\""));
}
