//! Rollups and formulas: computed for each reader, kept out of cells, checked
//! when saved, and following renamed properties.
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
fn col(id: &str, name: &str, kind: &str) -> Value {
    json!({"id":id,"name":name,"kind":kind,"options":[]})
}

#[tokio::test]
async fn rollups_and_formulas_are_computed_for_each_reader() {
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
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Books, in a folder the reader cannot open.
    let (_, private) = api(&router, "POST", "folders", &owner, json!({"name":"Private","workspace_id":boot["workspaces"].as_array().unwrap().iter().find(|w| w["name"]=="Team").unwrap()["id"]})).await;
    let (status, books) = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id":private["id"],"name":"Books","columns":[col("title","Title","text"),col("pages","Pages","number"),col("done","Finished","date")]}),
    )
    .await;
    assert_eq!(status, 200, "{books}");
    let bid = books["id"].as_str().unwrap().to_string();
    let mut ids = vec![];
    for (title, pages, done) in [
        ("Dune", 600, "2026-03-01"),
        ("Emma", 400, "2025-12-31"),
        ("Nil", 0, ""),
    ] {
        let mut cells = json!({"title":title,"pages":pages});
        if !done.is_empty() {
            cells["done"] = json!(done);
        }
        let (_, row) = api(
            &router,
            "POST",
            &format!("databases/{bid}/rows"),
            &owner,
            json!({"cells":cells}),
        )
        .await;
        ids.push(row["id"].as_str().unwrap().to_string());
    }

    // Shelves link to books and roll them up.
    let rollup = |id: &str, name: &str, property: &str, function: &str| json!({"id":id,"name":name,"kind":"rollup","options":[],"rollup":{"relation":"books","property":property,"function":function}});
    let mut columns = vec![
        col("name", "Shelf", "text"),
        json!({"id":"books","name":"Books","kind":"relation","options":[],"target":bid}),
        rollup("count", "Count", "", "count"),
        rollup("pages", "Pages", "pages", "sum"),
        rollup("avg", "Average", "pages", "average"),
        rollup("last", "Last read", "done", "latest"),
        rollup("titles", "Titles", "title", "list"),
        json!({"id":"per","name":"Per book","kind":"formula","options":[],"formula":"round(prop(\"Pages\") / prop(\"Count\"), 1)"}),
        json!({"id":"label","name":"Label","kind":"formula","options":[],"formula":"prop(\"Shelf\") + \": \" + prop(\"Per book\") + \" pages\""}),
    ];
    let (status, shelves) = api(
        &router,
        "POST",
        "databases",
        &owner,
        json!({"folder_id":folder,"name":"Shelves","columns":columns}),
    )
    .await;
    assert_eq!(status, 200, "{shelves}");
    let sid = shelves["id"].as_str().unwrap().to_string();
    let (_, row) = api(
        &router,
        "POST",
        &format!("databases/{sid}/rows"),
        &owner,
        json!({"cells":{"name":"Fiction","books":[ids[0], ids[1], ids[2]]}}),
    )
    .await;
    let c = &row["computed"];
    assert_eq!(c["count"], 3, "{row}");
    assert_eq!(c["pages"], 1000);
    assert!((c["avg"].as_f64().unwrap() - 1000.0 / 3.0).abs() < 1e-9);
    assert_eq!(c["last"], "2026-03-01");
    assert_eq!(c["titles"], json!(["Dune", "Emma", "Nil"]));
    assert_eq!(c["per"], 333.3);
    assert_eq!(c["label"], "Fiction: 333.3 pages");
    assert!(
        row["cells"].get("count").is_none(),
        "computed values are not cells"
    );

    // A change to the record recomputes; computed properties cannot be written.
    let (_, row) = api(
        &router,
        "PATCH",
        &format!("databases/{sid}/rows/{}", row["id"].as_str().unwrap()),
        &owner,
        json!({"revision":row["revision"],"cells":{"books":[ids[1]]}}),
    )
    .await;
    assert_eq!(row["computed"]["label"], "Fiction: 400 pages");
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("databases/{sid}/rows/{}", row["id"].as_str().unwrap()),
        &owner,
        json!({"revision":row["revision"],"cells":{"count":5}}),
    )
    .await;
    assert_eq!(status, 400);

    // Someone who cannot open Books sees no rolled-up values, only the formula's text.
    let (_, reader_boot) = api(&router, "GET", "bootstrap", &reader, json!({})).await;
    api(
        &router,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id":folder,"user_id":reader_boot["user"]["id"],"role":"viewer"}),
    )
    .await;
    let (_, seen) = api(
        &router,
        "GET",
        &format!("databases/{sid}"),
        &reader,
        json!({}),
    )
    .await;
    let c = &seen["rows"][0]["computed"];
    assert!(c["pages"].is_null() && c["count"].is_null(), "{c}");
    assert_eq!(c["label"], "Fiction:  pages");

    // Renaming a property updates formulas that use it; mistakes are explained.
    let (_, current) = api(
        &router,
        "GET",
        &format!("databases/{sid}"),
        &owner,
        json!({}),
    )
    .await;
    columns = current["columns"].as_array().unwrap().clone();
    columns.iter_mut().find(|c| c["id"] == "name").unwrap()["name"] = json!("Shelf name");
    let (status, renamed) = api(
        &router,
        "PATCH",
        &format!("databases/{sid}"),
        &owner,
        json!({"revision":current["revision"],"columns":columns}),
    )
    .await;
    assert_eq!(status, 200, "{renamed}");
    let label = renamed["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "label")
        .unwrap();
    assert!(label["formula"]
        .as_str()
        .unwrap()
        .starts_with("prop(\"Shelf name\")"));
    assert_eq!(
        renamed["rows"][0]["computed"]["label"],
        "Fiction: 400 pages"
    );
    // A rollup added later finds the relation that was already there.
    let mut cols = renamed["columns"].as_array().unwrap().clone();
    cols.push(json!({"id":"most","name":"Longest","kind":"rollup","options":[],"rollup":{"relation":"books","property":"pages","function":"max"}}));
    let (status, added) = api(
        &router,
        "PATCH",
        &format!("databases/{sid}"),
        &owner,
        json!({"revision":renamed["revision"],"columns":cols}),
    )
    .await;
    assert_eq!(status, 200, "{added}");
    assert_eq!(added["rows"][0]["computed"]["most"], 400);
    let renamed = added;
    for (bad, says) in [
        (
            json!({"id":"f","name":"F","kind":"formula","options":[],"formula":"prop(\"Nope\") + 1"}),
            "no property",
        ),
        (
            json!({"id":"f","name":"F","kind":"formula","options":[],"formula":"1 +"}),
            "ends too early",
        ),
        (
            json!({"id":"r","name":"R","kind":"rollup","options":[],"rollup":{"relation":"name","property":"","function":"count"}}),
            "relation property",
        ),
        (
            json!({"id":"r","name":"R","kind":"rollup","options":[],"rollup":{"relation":"books","property":"pages","function":"median"}}),
            "Unknown rollup",
        ),
        (
            json!({"id":"r","name":"R","kind":"rollup","options":[],"rollup":{"relation":"books","property":"gone","function":"sum"}}),
            "combines a property",
        ),
    ] {
        let mut cols = renamed["columns"].as_array().unwrap().clone();
        cols.push(bad.clone());
        let (status, err) = api(
            &router,
            "PATCH",
            &format!("databases/{sid}"),
            &owner,
            json!({"revision":renamed["revision"],"columns":cols}),
        )
        .await;
        assert_eq!(status, 400, "{bad}");
        assert!(err["error"].as_str().unwrap().contains(says), "{err}");
    }
}

#[tokio::test]
async fn rollups_of_rollups_and_formulas_over_related_records() {
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
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let create = |name: &'static str, columns: Value| {
        let (router, owner, folder) = (router.clone(), owner.clone(), folder.clone());
        async move {
            let (status, db) = api(
                &router,
                "POST",
                "databases",
                &owner,
                json!({"folder_id": folder, "name": name, "columns": columns}),
            )
            .await;
            assert_eq!(status, 200, "{db}");
            db["id"].as_str().unwrap().to_string()
        }
    };
    let add = |db: String, cells: Value| {
        let (router, owner) = (router.clone(), owner.clone());
        async move {
            let (_, row) = api(
                &router,
                "POST",
                &format!("databases/{db}/rows"),
                &owner,
                json!({"cells": cells}),
            )
            .await;
            row
        }
    };

    // Tasks with hours, and a cost worked out by a formula.
    let tasks = create(
        "Tasks",
        json!([col("t", "Task", "text"), col("hours", "Hours", "number"), {"id":"status","name":"Status","kind":"select","options":["Open","Done"]}, {"id":"cost","name":"Cost","kind":"formula","options":[],"formula":"prop(\"Hours\") * 10"}]),
    )
    .await;
    let mut ids = vec![];
    for (t, h, s) in [
        ("Plan", 2, "Done"),
        ("Build", 5, "Open"),
        ("Test", 3, "Open"),
    ] {
        let row = add(tasks.clone(), json!({"t": t, "hours": h, "status": s})).await;
        ids.push(row["id"].as_str().unwrap().to_string());
    }
    // Projects sum their tasks' costs (a rollup of a formula), and use the
    // tasks' properties in formulas.
    let projects = create(
        "Projects",
        json!([
            col("p", "Project", "text"),
            {"id":"tasks","name":"Tasks","kind":"relation","options":[],"target":tasks},
            {"id":"cost","name":"Cost","kind":"rollup","options":[],"rollup":{"relation":"tasks","property":"cost","function":"sum"}},
            {"id":"hours","name":"Hours","kind":"formula","options":[],"formula":"sum(related(\"Tasks\", \"Hours\"))"},
            {"id":"avg","name":"Average","kind":"formula","options":[],"formula":"average(related(\"Tasks\", \"Hours\"))"},
            {"id":"states","name":"States","kind":"formula","options":[],"formula":"join(unique(related(\"Tasks\", \"Status\")), \", \")"},
            {"id":"first","name":"First","kind":"formula","options":[],"formula":"first(related(\"tasks\", \"task\"))"},
            {"id":"open","name":"Open","kind":"formula","options":[],"formula":"length(related(\"Tasks\", \"Status\")) - 1"}
        ]),
    )
    .await;
    let row = add(
        projects.clone(),
        json!({"p": "Launch", "tasks": ids.clone()}),
    )
    .await;
    let c = &row["computed"];
    assert_eq!(c["cost"], 100, "{row}");
    assert_eq!(c["hours"], 10);
    assert!((c["avg"].as_f64().unwrap() - 10.0 / 3.0).abs() < 1e-9);
    assert_eq!(c["states"], "Done, Open");
    assert_eq!(c["first"], "Plan");
    assert_eq!(c["open"], 2);
    let launch = row["id"].as_str().unwrap().to_string();

    // Clients roll up their projects' rollup: a rollup of a rollup.
    let clients = create(
        "Clients",
        json!([
            col("c", "Client", "text"),
            {"id":"projects","name":"Projects","kind":"relation","options":[],"target":projects},
            {"id":"total","name":"Total","kind":"rollup","options":[],"rollup":{"relation":"projects","property":"cost","function":"sum"}}
        ]),
    )
    .await;
    let row = add(clients.clone(), json!({"c": "Acme", "projects": [launch]})).await;
    assert_eq!(row["computed"]["total"], 100, "{row}");

    // A loop (tasks rolling up the clients that roll them up) ends, with no value.
    let (_, base) = api(
        &router,
        "GET",
        &format!("databases/{tasks}"),
        &owner,
        json!({}),
    )
    .await;
    let mut columns = base["columns"].as_array().unwrap().clone();
    columns
        .push(json!({"id":"back","name":"Back","kind":"relation","options":[],"target":clients}));
    columns.push(json!({"id":"loop","name":"Loop","kind":"rollup","options":[],"rollup":{"relation":"back","property":"total","function":"sum"}}));
    let (status, updated) = api(
        &router,
        "PATCH",
        &format!("databases/{tasks}"),
        &owner,
        json!({"columns":columns,"revision":base["revision"]}),
    )
    .await;
    assert_eq!(status, 200, "{updated}");
    let (status, again) = api(
        &router,
        "GET",
        &format!("databases/{clients}"),
        &owner,
        json!({}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(again["rows"][0]["computed"]["total"], 100);
}
