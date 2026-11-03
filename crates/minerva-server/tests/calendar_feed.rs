//! The calendar feed: a secret address with someone's reminders, `⏰` times
//! and journal days, which can be replaced or turned off.
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
                .header("host", "notes.example.com")
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
/// The feed at a URL the API returned, fetched without signing in.
async fn fetch(router: &Router, url: &str) -> (u16, String) {
    let path = url.trim_start_matches("http://notes.example.com");
    send(router, "GET", path, "", Value::Null).await
}

#[tokio::test]
async fn feed_holds_what_its_owner_sees_and_can_be_replaced() {
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
    let other = common::member(&router, &root, "other@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let wid = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = api(
        &router,
        "POST",
        "reminders",
        &owner,
        json!({"workspace_id": wid, "title": "Renew passport, now", "category": "Admin",
               "at": 1_793_091_600u64, "repeat": {"every": 2, "unit": "year"}}),
    )
    .await;
    assert_eq!(status, 200);
    api(
        &router,
        "POST",
        "notes",
        &owner,
        json!({"folder_id": folder, "title": "Errands",
               "markdown": "- [ ] **Call** the bank ⏰2026-10-27 09:00"}),
    )
    .await;
    let (status, day) = api(
        &router,
        "POST",
        "journal",
        &owner,
        json!({"workspace_id": wid, "date": "2026-11-03"}),
    )
    .await;
    assert_eq!(status, 200, "{day}");

    // Off until asked for.
    let (_, state) = api(&router, "GET", "calendar-feed", &owner, json!({})).await;
    assert_eq!(state["enabled"], false);
    let (status, made) = api(
        &router,
        "POST",
        "calendar-feed",
        &owner,
        json!({"journal": true}),
    )
    .await;
    assert_eq!(status, 200, "{made}");
    let url = made["url"].as_str().unwrap().to_string();
    assert!(
        url.starts_with("http://notes.example.com/calendar/mnv_cal_"),
        "{url}"
    );
    let (_, state) = api(&router, "GET", "calendar-feed", &owner, json!({})).await;
    assert_eq!(state["enabled"], true);
    assert!(state["url"].is_null(), "the address is shown once");

    let (status, ics) = fetch(&router, &url).await;
    assert_eq!(status, 200);
    assert!(
        ics.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"),
        "{ics}"
    );
    assert!(ics.ends_with("END:VCALENDAR\r\n"));
    assert!(ics.contains("SUMMARY:Renew passport\\, now\r\n"), "{ics}");
    assert!(ics.contains("DTSTART:20261027T090000Z\r\n"));
    assert!(ics.contains("RRULE:FREQ=YEARLY;INTERVAL=2\r\n"));
    assert!(ics.contains("CATEGORIES:Admin\r\n"));
    assert!(ics.contains("SUMMARY:Call the bank\r\n"), "{ics}");
    assert!(
        ics.contains("DTSTART:20261027T090000\r\n"),
        "a floating local time"
    );
    assert!(ics.contains("DTSTART;VALUE=DATE:20261103\r\n"), "{ics}");
    assert!(ics.contains("BEGIN:VALARM"));
    assert!(ics.lines().all(|l| l.len() <= 75), "lines are folded");

    // Without journal days, and nothing of anyone else's.
    api(
        &router,
        "PUT",
        "calendar-feed",
        &owner,
        json!({"journal": false}),
    )
    .await;
    let (_, ics) = fetch(&router, &url).await;
    assert!(!ics.contains("VALUE=DATE"));
    let (_, theirs) = api(&router, "POST", "calendar-feed", &other, json!({})).await;
    let (_, ics) = fetch(&router, theirs["url"].as_str().unwrap()).await;
    assert!(
        !ics.contains("Call the bank") && !ics.contains("Renew passport"),
        "{ics}"
    );

    // A new address replaces the old one; turning it off ends it.
    let (_, again) = api(&router, "POST", "calendar-feed", &owner, json!({})).await;
    assert_eq!(fetch(&router, &url).await.0, 404);
    let newer = again["url"].as_str().unwrap().to_string();
    assert_eq!(fetch(&router, &newer).await.0, 200);
    api(&router, "DELETE", "calendar-feed", &owner, json!({})).await;
    assert_eq!(fetch(&router, &newer).await.0, 404);
    assert_eq!(fetch(&router, "/calendar/mnv_cal_nope.ics").await.0, 404);

    // Administrators have no notes; personal API tokens cannot manage the feed.
    let (status, _) = api(&router, "POST", "calendar-feed", &root, json!({})).await;
    assert_eq!(status, 403);
    let (_, token) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name": "script"}),
    )
    .await;
    let (status, _) = api(
        &router,
        "POST",
        "calendar-feed",
        token["token"].as_str().unwrap(),
        json!({}),
    )
    .await;
    assert_eq!(status, 403);
}
