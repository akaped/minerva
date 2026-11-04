//! Decks shared with a workspace: everyone studies them with their own
//! progress, only editors change them; plus a retention per deck and the
//! time spent on reviews.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
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
fn card<'a>(deck: &'a Value, front: &str) -> &'a Value {
    deck["deck"]["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["front"] == front)
        .unwrap()
}

#[tokio::test]
async fn shared_decks_keep_each_persons_progress() {
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
    let owner = common::member(&r, &admin, "owner@test.local").await;
    let reader = common::account(&r, &admin, "reader@test.local").await;
    let editor = common::account(&r, &admin, "editor@test.local").await;
    let stranger = common::account(&r, &admin, "stranger@test.local").await;
    let (_, boot) = api(&r, "GET", "bootstrap", &owner, json!({})).await;
    let team = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let personal = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["personal"] == true)
        .unwrap()["id"]
        .clone();
    for (token, role) in [(&reader, "viewer"), (&editor, "editor")] {
        let (_, them) = api(&r, "GET", "bootstrap", token, json!({})).await;
        let (status, _) = api(
            &r,
            "PUT",
            "workspace-access",
            &owner,
            json!({"workspace_id": team, "user_id": them["user"]["id"], "role": role}),
        )
        .await;
        assert_eq!(status, 200);
    }

    // Shared with a workspace, never with the personal one, and not following notes.
    let (status, _) = api(
        &r,
        "POST",
        "flashcards/decks",
        &owner,
        json!({"name":"x","workspace_id": personal}),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = api(
        &r,
        "POST",
        "flashcards/decks",
        &owner,
        json!({"name":"x","workspace_id": team, "source": {"kind":"workspace","id": team}}),
    )
    .await;
    assert_eq!(status, 400);
    let (status, deck) = api(
        &r,
        "POST",
        "flashcards/decks",
        &owner,
        json!({"name":"Team words","workspace_id": team}),
    )
    .await;
    assert_eq!(status, 201, "{deck}");
    let did = deck["id"].as_str().unwrap().to_string();
    let path = format!("flashcards/decks/{did}");
    let (_, deck) = api(
        &r,
        "POST",
        &format!("{path}/cards"),
        &owner,
        json!({"revision": deck["revision"], "cards":[{"front":"ciao","back":"hello"},{"front":"grazie","back":"thanks"}]}),
    )
    .await;

    // Members see it; the reader only studies, the editor changes it too.
    let (_, list) = api(&r, "GET", "flashcards/decks", &reader, json!({})).await;
    let row = list["decks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == did)
        .unwrap()
        .clone();
    assert_eq!(row["can_edit"], false);
    assert_eq!(row["workspace_name"], "Team");
    let (_, list) = api(&r, "GET", "flashcards/decks", &stranger, json!({})).await;
    assert!(
        list["decks"].as_array().unwrap().is_empty(),
        "outsiders do not see it"
    );
    let (status, _) = api(&r, "GET", &path, &stranger, json!({})).await;
    assert_eq!(status, 404);
    let (status, _) = api(
        &r,
        "POST",
        &format!("{path}/cards"),
        &reader,
        json!({"revision": deck["deck"]["revision"], "cards":[{"front":"no","back":"no"}]}),
    )
    .await;
    assert_eq!(status, 403);
    let (status, deck) = api(
        &r,
        "POST",
        &format!("{path}/cards"),
        &editor,
        json!({"revision": deck["deck"]["revision"], "cards":[{"front":"prego","back":"you're welcome"}]}),
    )
    .await;
    assert_eq!(status, 200, "{deck}");

    // The reader's review is theirs alone, with the time it took.
    let (_, theirs) = api(&r, "GET", &path, &reader, json!({})).await;
    assert_eq!(theirs["can_edit"], false);
    let ciao = card(&theirs, "ciao");
    let (status, theirs) = api(
        &r,
        "POST",
        &format!("{path}/cards/{}/review", ciao["id"].as_str().unwrap()),
        &reader,
        json!({"rating":"good","revision": ciao["revision"],"seconds": 4.2}),
    )
    .await;
    assert_eq!(status, 200, "{theirs}");
    assert_eq!(card(&theirs, "ciao")["reviews"], 1);
    let (_, mine) = api(&r, "GET", &path, &owner, json!({})).await;
    assert_eq!(
        card(&mine, "ciao")["reviews"],
        0,
        "the owner's progress is untouched"
    );
    // Pausing is personal too.
    let grazie = card(&theirs, "grazie");
    let (status, _) = api(
        &r,
        "PUT",
        &format!("{path}/cards/{}", grazie["id"].as_str().unwrap()),
        &reader,
        json!({"front":"changed","back":"changed","suspended":true,"revision": grazie["revision"]}),
    )
    .await;
    assert_eq!(status, 200);
    let (_, theirs) = api(&r, "GET", &path, &reader, json!({})).await;
    assert_eq!(card(&theirs, "grazie")["suspended"], true);
    let (_, mine) = api(&r, "GET", &path, &owner, json!({})).await;
    assert_eq!(card(&mine, "grazie")["suspended"], false);
    assert!(mine["deck"]["cards"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["front"] != "changed"));
    let (_, stats) = api(&r, "GET", "flashcards/stats", &reader, json!({})).await;
    assert_eq!(stats["seconds_30_days"], 4.0);
    assert_eq!(stats["seconds_per_review"], 4.2);

    // Pictures belong to the deck: members see them, only editors add them.
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    let (status, _) = send(&r, "POST", &format!("{path}/media"), &reader, png.clone()).await;
    assert_eq!(status, 403);
    let (_, picture) = send(&r, "POST", &format!("{path}/media"), &owner, png).await;
    let media = picture["markdown"]
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
                .header("authorization", format!("Bearer {reader}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);

    // A deck's own retention lengthens or shortens everyone's intervals.
    let good = |d: &Value| card(d, "prego")["next"]["good"].as_u64().unwrap();
    let before = good(&mine);
    let (status, relaxed) = api(
        &r,
        "PUT",
        &path,
        &owner,
        json!({"name":"Team words","description":"","retention":0.8,"revision": mine["deck"]["revision"]}),
    )
    .await;
    assert_eq!(status, 200, "{relaxed}");
    assert!(good(&relaxed) > before);

    // A personal deck can be shared later; its owner keeps their progress.
    let (_, own) = api(
        &r,
        "POST",
        "flashcards/decks",
        &owner,
        json!({"name":"Mine"}),
    )
    .await;
    let own_path = format!("flashcards/decks/{}", own["id"].as_str().unwrap());
    let (_, own) = api(
        &r,
        "POST",
        &format!("{own_path}/cards"),
        &owner,
        json!({"revision": own["revision"], "cards":[{"front":"uno","back":"one"}]}),
    )
    .await;
    let uno = card(&own, "uno");
    let (_, own) = api(
        &r,
        "POST",
        &format!("{own_path}/cards/{}/review", uno["id"].as_str().unwrap()),
        &owner,
        json!({"rating":"easy","revision": uno["revision"]}),
    )
    .await;
    let (status, shared) = api(
        &r,
        "PUT",
        &own_path,
        &owner,
        json!({"name":"Mine","description":"","workspace_id": team,"revision": own["deck"]["revision"]}),
    )
    .await;
    assert_eq!(status, 200, "{shared}");
    assert_eq!(card(&shared, "uno")["reviews"], 1);
    let (_, seen) = api(&r, "GET", &own_path, &reader, json!({})).await;
    assert_eq!(card(&seen, "uno")["reviews"], 0, "new to the reader");
}
