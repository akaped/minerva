//! Importing Anki decks: legacy and current packages built the way Anki writes
//! them, plain-text exports, scheduling, and refusal of other files.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::io::Write;
use tower::ServiceExt;
mod common;

async fn send(
    r: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
/// A member's session: the administrator only creates the account.
async fn login(r: &Router) -> String {
    let body = json!({"email":"admin@test.local","password":"test-password-1234"}).to_string();
    let admin: String = send(r, "POST", "login", "", body.into_bytes()).await.1["token"]
        .as_str()
        .unwrap()
        .into();
    common::member(r, &admin, "owner@test.local").await
}
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    for (name, data) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}
fn database(build: impl FnOnce(&Connection)) -> Vec<u8> {
    let path = std::env::temp_dir().join(format!("anki-fixture-{}.db", uuid::Uuid::new_v4()));
    {
        let db = Connection::open(&path).unwrap();
        build(&db);
    }
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    bytes
}
const CARDS: &str = "CREATE TABLE notes(id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT);
    CREATE TABLE cards(id INTEGER PRIMARY KEY, nid INTEGER, did INTEGER, odid INTEGER, ord INTEGER,
        type INTEGER, queue INTEGER, due INTEGER, ivl INTEGER, reps INTEGER, lapses INTEGER);";
const CREATED: i64 = 1_600_000_000;
/// The start of a PNG file: enough to be recognised as a picture.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

/// Anki 2.1 before 2.1.28: note types and decks as JSON in `col`.
fn legacy_collection() -> Vec<u8> {
    database(|db| {
        db.execute_batch(&format!(
            "CREATE TABLE col(crt INTEGER, models TEXT, decks TEXT); {CARDS}"
        ))
        .unwrap();
        let models = json!({
            "1": {"type":0,"flds":[{"name":"Front","ord":0},{"name":"Back","ord":1}],
                  "tmpls":[{"ord":0,"qfmt":"{{Front}}","afmt":"{{FrontSide}}<hr id=answer>{{Back}}"},
                           {"ord":1,"qfmt":"{{Back}}","afmt":"{{FrontSide}}<hr id=answer>{{Front}}"}]},
            "2": {"type":1,"flds":[{"name":"Text","ord":0},{"name":"Back Extra","ord":1}],
                  "tmpls":[{"ord":0,"qfmt":"<div style=\"display: none\">{{Text}}</div><button onclick=\"all()\">Show All Clozes</button>{{cloze:Text}}","afmt":"{{cloze:Text}}<br>{{Back Extra}}<script>all()</script>"}]}
        });
        let decks = json!({"1":{"name":"Default"},"10":{"name":"Geography::Capitals"}});
        db.execute(
            "INSERT INTO col VALUES(?1, ?2, ?3)",
            params![CREATED, models.to_string(), decks.to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO notes VALUES(100, 1, ?1)",
            ["<b>France</b>\u{1f}Paris&nbsp;<img src=\"eiffel.jpg\">"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO notes VALUES(101, 2, ?1)",
            ["{{c1::Rome}} is the capital of {{c2::Italy::country}}.\u{1f}<div>Since <a href=\"https://en.wikipedia.org/wiki/Rome\">1871</a></div>"],
        )
        .unwrap();
        // A review card due 3 days after creation with a 10-day interval, its
        // reverse suspended, and the cloze cards new.
        db.execute_batch(
            "INSERT INTO cards VALUES(1, 100, 10, 0, 0, 2, 2, 3, 10, 7, 1);
             INSERT INTO cards VALUES(2, 100, 10, 0, 1, 0, -1, 0, 0, 0, 0);
             INSERT INTO cards VALUES(3, 101, 10, 0, 0, 0, 0, 1, 0, 0, 0);
             INSERT INTO cards VALUES(4, 101, 10, 0, 1, 0, 0, 2, 0, 0, 0);",
        )
        .unwrap();
    })
}
/// A protocol buffer with the question (1) and answer (2) template strings.
fn template(question: &str, answer: &str) -> Vec<u8> {
    let mut out = vec![];
    for (field, value) in [(1u8, question), (2, answer)] {
        out.push(field << 3 | 2);
        let mut len = value.len();
        loop {
            let byte = (len & 0x7f) as u8;
            len >>= 7;
            out.push(if len > 0 { byte | 0x80 } else { byte });
            if len == 0 {
                break;
            }
        }
        out.extend_from_slice(value.as_bytes());
    }
    out
}
/// Anki 2.1.50 and later: tables for note types and decks, compressed.
fn current_collection() -> Vec<u8> {
    let db = database(|db| {
        db.execute_batch(&format!(
            "CREATE TABLE col(crt INTEGER); CREATE TABLE notetypes(id INTEGER, name TEXT);
             CREATE TABLE fields(ntid INTEGER, ord INTEGER, name TEXT);
             CREATE TABLE templates(ntid INTEGER, ord INTEGER, name TEXT, config BLOB);
             CREATE TABLE decks(id INTEGER, name TEXT); {CARDS}"
        ))
        .unwrap();
        db.execute("INSERT INTO col VALUES(?1)", [CREATED]).unwrap();
        db.execute_batch(
            "INSERT INTO notetypes VALUES(5, 'Vocabulary');
             INSERT INTO fields VALUES(5, 0, 'Word'); INSERT INTO fields VALUES(5, 1, 'Meaning');
             INSERT INTO fields VALUES(5, 2, 'Example');",
        )
        .unwrap();
        let config = template(
            "<div class=word>{{Word}}</div>",
            "{{FrontSide}}<hr id=answer>{{Meaning}}{{#Example}}<br><i>{{Example}}</i>{{/Example}}",
        );
        db.execute("INSERT INTO templates VALUES(5, 0, 'Card 1', ?1)", [config])
            .unwrap();
        db.execute("INSERT INTO decks VALUES(20, ?1)", ["Spanish\u{1f}Verbs"])
            .unwrap();
        db.execute(
            "INSERT INTO notes VALUES(200, 5, ?1)",
            ["hablar\u{1f}to speak\u{1f}Hablo español.<img src=\"verb.png\">"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO notes VALUES(201, 5, ?1)",
            ["comer\u{1f}to eat\u{1f}"],
        )
        .unwrap();
        db.execute_batch(
            "INSERT INTO cards VALUES(1, 200, 20, 0, 0, 0, 0, 1, 0, 0, 0);
             INSERT INTO cards VALUES(2, 201, 20, 0, 0, 0, 0, 2, 0, 0, 0);",
        )
        .unwrap();
    });
    ruzstd::encoding::compress_to_vec(&db[..], ruzstd::encoding::CompressionLevel::Fastest)
}

#[tokio::test]
async fn anki_packages_and_text_exports_become_decks() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let token = login(&r).await;

    // Legacy package, with review progress.
    let package = zip(&[
        ("collection.anki2", &legacy_collection()),
        ("media", br#"{"0":"eiffel.jpg","1":"unused.png"}"#),
        ("0", PNG),
        ("1", PNG),
    ]);
    let (status, result) = send(
        &r,
        "POST",
        "flashcards/import?name=geo.apkg",
        &token,
        package.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["cards"], 4);
    assert_eq!(result["decks"][0]["name"], "Geography › Capitals");
    let id = result["decks"][0]["id"].as_str().unwrap();
    let (_, deck) = send(&r, "GET", &format!("flashcards/decks/{id}"), &token, vec![]).await;
    let cards = deck["deck"]["cards"].as_array().unwrap();
    let pair = |i: usize| {
        (
            cards[i]["front"].as_str().unwrap(),
            cards[i]["back"].as_str().unwrap(),
        )
    };
    // The picture comes along, stored once for the deck and shown by both cards.
    let (front, back) = pair(0);
    assert_eq!(front, "France");
    let media = back
        .strip_prefix("Paris ![](media:")
        .and_then(|m| m.strip_suffix(')'))
        .unwrap();
    assert_eq!(pair(1), (back, "France"), "the reversed card");
    let picture = r
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/flashcards/media/{media}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(picture.status(), StatusCode::OK);
    assert_eq!(picture.headers()["content-type"], "image/png");
    assert_eq!(
        &picture.into_body().collect().await.unwrap().to_bytes()[..],
        PNG
    );
    assert_eq!(
        pair(2),
        (
            "[...] is the capital of Italy.",
            "Rome is the capital of Italy.\nSince [1871](https://en.wikipedia.org/wiki/Rome)"
        )
    );
    assert_eq!(pair(3).0, "Rome is the capital of [country].");
    assert_eq!(cards[0]["interval"], 10 * 86400);
    assert_eq!(cards[0]["due"], CREATED + 3 * 86400);
    assert_eq!(
        (cards[0]["reviews"].as_u64(), cards[0]["lapses"].as_u64()),
        (Some(7), Some(1))
    );
    assert_eq!(cards[1]["suspended"], true);
    assert_eq!(cards[2]["interval"], 0);
    assert_eq!(deck["deck"]["description"], "Imported from Anki (geo.apkg)");

    // Without progress, every card starts as new.
    let (_, fresh) = send(
        &r,
        "POST",
        "flashcards/import?name=geo.apkg&progress=0",
        &token,
        package,
    )
    .await;
    let id = fresh["decks"][0]["id"].as_str().unwrap();
    let (_, deck) = send(&r, "GET", &format!("flashcards/decks/{id}"), &token, vec![]).await;
    let first = &deck["deck"]["cards"][0];
    assert_eq!(
        (
            first["interval"].as_u64(),
            first["reviews"].as_u64(),
            first["suspended"].as_bool()
        ),
        (Some(0), Some(0), Some(false))
    );

    // Current package: the compressed collection wins over the legacy stub.
    let stub = database(|db| {
        db.execute_batch(&format!(
            "CREATE TABLE col(crt INTEGER, models TEXT, decks TEXT); {CARDS}"
        ))
        .unwrap();
        db.execute("INSERT INTO col VALUES(1, '{}', '{}')", [])
            .unwrap();
    });
    // Since Anki 2.1.50 the media list (entries of name = 1) and files are
    // compressed too.
    let list = [&[0x0a, 10, 0x0a, 8][..], b"verb.png"].concat();
    let compress = |b: &[u8]| {
        ruzstd::encoding::compress_to_vec(b, ruzstd::encoding::CompressionLevel::Fastest)
    };
    let package = zip(&[
        ("collection.anki2", &stub),
        ("collection.anki21b", &current_collection()),
        ("media", &compress(&list)),
        ("0", &compress(PNG)),
    ]);
    let (status, result) = send(
        &r,
        "POST",
        "flashcards/import?name=spanish.apkg",
        &token,
        package,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["decks"][0]["name"], "Spanish › Verbs");
    let id = result["decks"][0]["id"].as_str().unwrap();
    let (_, deck) = send(&r, "GET", &format!("flashcards/decks/{id}"), &token, vec![]).await;
    let cards = deck["deck"]["cards"].as_array().unwrap();
    assert_eq!(cards[0]["front"], "hablar");
    assert!(
        cards[0]["back"]
            .as_str()
            .unwrap()
            .starts_with("to speak\nHablo español.![](media:"),
        "{}",
        cards[0]["back"]
    );
    assert_eq!(cards[1]["back"], "to eat", "empty sections are left out");

    // A plain-text export, one deck per deck column.
    let export = "#separator:tab\n#html:true\n#deck column:3\ngato\t<b>cat</b>\tAnimals\nperro\tdog\tAnimals\n";
    let (status, result) = send(
        &r,
        "POST",
        "flashcards/import?name=animals.txt",
        &token,
        export.into(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        (
            result["decks"][0]["name"].as_str(),
            result["cards"].as_u64()
        ),
        (Some("Animals"), Some(2))
    );

    // Other files and signed-out requests are refused.
    let (status, error) = send(
        &r,
        "POST",
        "flashcards/import?name=x.apkg",
        &token,
        zip(&[("readme.txt", b"hi")]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"]
        .as_str()
        .unwrap()
        .contains("no Anki collection"));
    let (status, _) = send(
        &r,
        "POST",
        "flashcards/import?name=x.bin",
        &token,
        vec![0xff, 0xfe, 0x00],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &r,
        "POST",
        "flashcards/import?name=geo.apkg",
        "",
        zip(&[("collection.anki2", &legacy_collection())]),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
