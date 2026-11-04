//! Spaced repetition fitted to the learner: review history (from Anki or from
//! studying), statistics, desired retention, optimizing, and media cards.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::io::Write;
use tower::ServiceExt;
mod common;

async fn send(r: &Router, method: &str, path: &str, token: &str, body: Vec<u8>) -> (u16, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("x-minerva-client", "1")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body))
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
async fn api(r: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    send(r, method, path, token, body.to_string().into_bytes()).await
}

const DAY: i64 = 86400;
/// An Anki package of 150 cards, each reviewed five times at growing
/// intervals, almost always recalled; and a card with a sound.
fn package() -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("anki-memory-{}.db", uuid::Uuid::new_v4()));
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE col(crt INTEGER, models TEXT, decks TEXT);
             CREATE TABLE notes(id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT);
             CREATE TABLE cards(id INTEGER PRIMARY KEY, nid INTEGER, did INTEGER, odid INTEGER, ord INTEGER,
                 type INTEGER, queue INTEGER, due INTEGER, ivl INTEGER, reps INTEGER, lapses INTEGER);
             CREATE TABLE revlog(id INTEGER PRIMARY KEY, cid INTEGER, ease INTEGER, type INTEGER);",
        )
        .unwrap();
        let models = json!({"1": {"type":0,"flds":[{"name":"Front","ord":0},{"name":"Back","ord":1}],
            "tmpls":[{"ord":0,"qfmt":"{{Front}}","afmt":"{{FrontSide}}<hr id=answer>{{Back}}"}]}});
        db.execute(
            "INSERT INTO col VALUES(1600000000, ?1, ?2)",
            params![
                models.to_string(),
                json!({"1":{"name":"Words"}}).to_string()
            ],
        )
        .unwrap();
        let start = 1_700_000_000i64;
        for card in 0..150i64 {
            db.execute(
                "INSERT INTO notes VALUES(?1, 1, ?2)",
                params![card, format!("word {card}\u{1f}meaning {card}")],
            )
            .unwrap();
            db.execute(
                "INSERT INTO cards VALUES(?1, ?1, 1, 0, 0, 2, 2, 5000, 140, 6, 0)",
                params![card],
            )
            .unwrap();
            let mut at = start + card * 3600;
            for (i, days) in [0i64, 3, 9, 25, 60, 140].iter().enumerate() {
                at += days * DAY;
                let ease = if (card + i as i64) % 23 == 0 { 1 } else { 3 };
                db.execute(
                    "INSERT INTO revlog VALUES(?1, ?2, ?3, 1)",
                    params![at * 1000 + i as i64, card, ease],
                )
                .unwrap();
            }
        }
        db.execute(
            "INSERT INTO notes VALUES(999, 1, ?1)",
            ["Listen [sound:hello.mp3]\u{1f}Hello"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO cards VALUES(999, 999, 1, 0, 0, 0, 0, 1, 0, 0, 0)",
            [],
        )
        .unwrap();
    }
    let collection = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    for (name, data) in [
        ("collection.anki21", collection.as_slice()),
        ("media", br#"{"0":"hello.mp3"}"#.as_slice()),
        ("0", b"ID3\x04\0\0\0\0\0\0sound".as_slice()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}

#[tokio::test]
async fn reviews_fit_the_scheduler_and_feed_statistics() {
    let r = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let admin = login["token"].as_str().unwrap().to_string();
    let me = common::member(&r, &admin, "learner@test.local").await;

    // Defaults until something is chosen or fitted.
    let (_, settings) = api(&r, "GET", "flashcards/settings", &me, json!({})).await;
    assert_eq!(settings["retention"], 0.9);
    assert_eq!(settings["optimized"], false);
    let (status, err) = api(&r, "POST", "flashcards/settings/optimize", &me, json!({})).await;
    assert_eq!(status, 400, "no history yet: {err}");

    // Anki's history comes along: logged, and each card's memory rebuilt.
    let (status, imported) = send(
        &r,
        "POST",
        "flashcards/import?name=words.apkg&progress=1",
        &me,
        package(),
    )
    .await;
    assert_eq!(status, 200, "{imported}");
    let deck_id = imported["decks"][0]["id"].as_str().unwrap().to_string();
    let (_, deck) = api(
        &r,
        "GET",
        &format!("flashcards/decks/{deck_id}"),
        &me,
        json!({}),
    )
    .await;
    let cards = deck["deck"]["cards"].as_array().unwrap();
    assert!(
        cards[0]["stability"].as_f64().unwrap() > 30.0,
        "{}",
        cards[0]
    );
    let sound = cards
        .iter()
        .find(|c| c["front"].as_str().unwrap().starts_with("Listen"))
        .unwrap();
    assert!(
        sound["front"].as_str().unwrap().contains("![audio](media:"),
        "{sound}"
    );
    let media = sound["front"]
        .as_str()
        .unwrap()
        .split("media:")
        .nth(1)
        .unwrap()[..36]
        .to_string();
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/flashcards/media/{media}"))
                .header("authorization", format!("Bearer {me}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "audio/mpeg");

    let (_, stats) = api(&r, "GET", "flashcards/stats?offset=60", &me, json!({})).await;
    assert_eq!(stats["reviews"], 900);
    assert_eq!(stats["cards"]["new"], 1);
    assert_eq!(stats["days"].as_array().unwrap().len(), 30);
    assert_eq!(stats["forecast"].as_array().unwrap().len(), 30);

    // Optimizing fits the weights to this history and keeps them.
    let (status, fitted) = api(&r, "POST", "flashcards/settings/optimize", &me, json!({})).await;
    assert_eq!(status, 200, "{fitted}");
    assert_eq!(fitted["optimized"], true);
    assert_eq!(fitted["fitted"]["kept"], true);
    assert!(
        fitted["fitted"]["loss_after"].as_f64() < fitted["fitted"]["loss_before"].as_f64(),
        "{fitted}"
    );

    // The desired retention moves the intervals.
    let (_, own) = api(&r, "POST", "flashcards/decks", &me, json!({"name":"Mine"})).await;
    let own_id = own["id"].as_str().unwrap().to_string();
    let (status, own) = api(
        &r,
        "POST",
        &format!("flashcards/decks/{own_id}/cards"),
        &me,
        json!({"revision":own["revision"],"cards":[{"front":"2+2","back":"4"}]}),
    )
    .await;
    assert_eq!(status, 200, "{own}");
    let good = |deck: &Value| deck["deck"]["cards"][0]["next"]["good"].as_u64().unwrap();
    let before = good(&own);
    let (status, _) = api(
        &r,
        "PUT",
        "flashcards/settings",
        &me,
        json!({"retention":0.8}),
    )
    .await;
    assert_eq!(status, 200);
    let (_, relaxed) = api(
        &r,
        "GET",
        &format!("flashcards/decks/{own_id}"),
        &me,
        json!({}),
    )
    .await;
    assert!(good(&relaxed) > before, "{} > {before}", good(&relaxed));
    let (status, _) = api(
        &r,
        "PUT",
        "flashcards/settings",
        &me,
        json!({"retention":0.5}),
    )
    .await;
    assert_eq!(status, 400);

    // Pictures and sounds can be added to a card.
    let (status, picture) = send(
        &r,
        "POST",
        &format!("flashcards/decks/{own_id}/media"),
        &me,
        b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec(),
    )
    .await;
    assert_eq!(status, 200, "{picture}");
    assert!(picture["markdown"]
        .as_str()
        .unwrap()
        .starts_with("![](media:"));
    let (status, _) = send(
        &r,
        "POST",
        &format!("flashcards/decks/{own_id}/media"),
        &me,
        b"plain".to_vec(),
    )
    .await;
    assert_eq!(status, 400);

    // Studying is logged: today's statistics show it.
    let card = relaxed["deck"]["cards"][0].clone();
    let (status, _) = api(
        &r,
        "POST",
        &format!(
            "flashcards/decks/{own_id}/cards/{}/review",
            card["id"].as_str().unwrap()
        ),
        &me,
        json!({"rating":"good","revision":card["revision"]}),
    )
    .await;
    assert_eq!(status, 200);
    let (_, today) = api(
        &r,
        "GET",
        &format!("flashcards/stats?deck={own_id}"),
        &me,
        json!({}),
    )
    .await;
    assert_eq!(today["reviews"], 1);
    assert_eq!(today["reviews_30_days"], 1);
    assert_eq!(today["streak"], 1);
    assert_eq!(today["studied_today"], true);
    assert_eq!(today["days"][29]["good"], 1);

    // A deleted deck takes its history with it.
    api(
        &r,
        "DELETE",
        &format!("flashcards/decks/{deck_id}"),
        &me,
        json!({"revision": deck["deck"]["revision"]}),
    )
    .await;
    let (_, after) = api(&r, "GET", "flashcards/stats", &me, json!({})).await;
    assert_eq!(after["reviews"], 1);
}
