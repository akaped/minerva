//! Database properties beyond the basics: multi-select, person, created and
//! edited times (kept by the server), and relations to another database.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
mod common;

async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn multi_select_person_times_and_relations() {
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
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let me = boot["user"]["id"].as_str().unwrap().to_string();

    let (_, books) = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id":folder,"name":"Books"}),
    )
    .await;
    let books_id = books["id"].as_str().unwrap().to_string();
    let book_name = books["columns"][0]["id"].as_str().unwrap().to_string();
    let (_, dune) = api(
        &router,
        "POST",
        &format!("databases/{books_id}/rows"),
        &owner,
        json!({"cells":{&book_name:"Dune"}}),
    )
    .await;
    assert!(
        dune["created_at"].as_u64().unwrap() > 0,
        "records keep when they were created"
    );

    let (_, reading) = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id":folder,"name":"Reading"}),
    )
    .await;
    let rid = reading["id"].as_str().unwrap().to_string();
    let mut columns = reading["columns"].as_array().unwrap().clone();
    columns.extend([
        json!({"id":"tags","name":"Tags","kind":"multi_select","options":["sci-fi","classic"]}),
        json!({"id":"who","name":"Reader","kind":"person"}),
        json!({"id":"made","name":"Created","kind":"created"}),
        json!({"id":"seen","name":"Edited","kind":"edited"}),
        json!({"id":"book","name":"Book","kind":"relation","target":books_id}),
    ]);
    let (status, reading) = api(
        &router,
        "PATCH",
        &format!("databases/{rid}"),
        &owner,
        json!({"revision":reading["revision"],"columns":columns}),
    )
    .await;
    assert_eq!(status, 200, "{reading}");
    let name = reading["columns"][0]["id"].as_str().unwrap().to_string();

    // Valid values are kept; the server's own times cannot be written.
    let (status, row) = api(
        &router,
        "POST",
        &format!("databases/{rid}/rows"),
        &owner,
        json!({"cells":{&name:"Spring","tags":["sci-fi","classic"],"who":me,"book":[dune["id"]]}}),
    )
    .await;
    assert_eq!(status, 200, "{row}");
    for bad in [
        json!({"tags":["poetry"]}),
        json!({"tags":["sci-fi","sci-fi"]}),
        json!({"tags":"sci-fi"}),
        json!({"made":"2026-01-01"}),
        json!({"seen":1}),
        json!({"book":"not-a-list"}),
        json!({"who":"<script>"}),
    ] {
        let (status, _) = api(
            &router,
            "POST",
            &format!("databases/{rid}/rows"),
            &owner,
            json!({"cells":bad}),
        )
        .await;
        assert_eq!(status, 400, "{bad}");
    }

    // Relations need a target, in this workspace, that the person can open.
    let (_, fresh) = api(
        &router,
        "GET",
        &format!("databases/{rid}"),
        &owner,
        json!({}),
    )
    .await;
    let mut broken = fresh["columns"].as_array().unwrap().clone();
    broken.push(json!({"id":"x","name":"X","kind":"relation"}));
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("databases/{rid}"),
        &owner,
        json!({"revision":fresh["revision"],"columns":broken}),
    )
    .await;
    assert_eq!(status, 400, "a relation without a target");
    let (_, other_boot) = api(&router, "GET", "bootstrap", &other, json!({})).await;
    let other_folder = common::team_folder(&other_boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, theirs) = api(
        &router,
        "POST",
        "databases",
        &other,
        json!({"folder_id":other_folder,"name":"Private"}),
    )
    .await;
    let mut foreign = fresh["columns"].as_array().unwrap().clone();
    foreign.push(json!({"id":"y","name":"Y","kind":"relation","target":theirs["id"]}));
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("databases/{rid}"),
        &owner,
        json!({"revision":fresh["revision"],"columns":foreign}),
    )
    .await;
    assert_eq!(status, 400, "a relation to a database one cannot open");

    // The people a person property can name.
    let (status, people) = api(
        &router,
        "GET",
        &format!("databases/{rid}/people"),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(status, 200);
    let names: Vec<_> = people
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        std::slice::from_ref(&me),
        "only members who can open it: {people}"
    );
    let (status, _) = api(
        &router,
        "GET",
        &format!("databases/{rid}/people"),
        &other,
        json!({}),
    )
    .await;
    assert_eq!(status, 403);

    // Removing an option takes it out of records, keeping the others.
    let (_, fresh) = api(
        &router,
        "GET",
        &format!("databases/{rid}"),
        &owner,
        json!({}),
    )
    .await;
    let mut fewer = fresh["columns"].as_array().unwrap().clone();
    for c in &mut fewer {
        if c["id"] == "tags" {
            c["options"] = json!(["sci-fi"]);
        }
    }
    let (_, after) = api(
        &router,
        "PATCH",
        &format!("databases/{rid}"),
        &owner,
        json!({"revision":fresh["revision"],"columns":fewer}),
    )
    .await;
    let kept = after["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == row["id"])
        .unwrap();
    assert_eq!(kept["cells"]["tags"], json!(["sci-fi"]), "{kept}");
    assert_eq!(kept["cells"]["who"], json!(me));
}
