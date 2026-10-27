//! Deleting databases, records, and properties; embedding live databases in notes.
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

#[tokio::test]
async fn databases_records_and_properties_delete_and_embed_live_in_notes() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let (_, carol) = api(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Carol","email":"carol@test.local","password":PASSWORD}),
    )
    .await;
    let carol_id = carol["id"].as_str().unwrap().to_string();
    let carol = login(&router, "carol@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &admin, json!({})).await;
    let wid = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let notes_folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, private) = api(
        &router,
        "POST",
        "folders",
        &admin,
        json!({"workspace_id":wid,"name":"Private"}),
    )
    .await;
    let private = private["id"].as_str().unwrap().to_string();

    // A task database with records.
    let (_, db) = api(
        &router,
        "POST",
        "databases",
        &admin,
        json!({"folder_id":private,"name":"My tasks"}),
    )
    .await;
    let did = db["id"].as_str().unwrap().to_string();
    let name = db["columns"][0]["id"].as_str().unwrap().to_string();
    let status_col = db["columns"][1]["id"].as_str().unwrap().to_string();
    let add = |title: &'static str, status: &'static str| {
        let router = router.clone();
        let admin = admin.clone();
        let (did, name, status_col) = (did.clone(), name.clone(), status_col.clone());
        async move {
            let (_, row) = api(
                &router,
                "POST",
                &format!("databases/{did}/rows"),
                &admin,
                json!({"cells":{name:title,status_col:status}}),
            )
            .await;
            row["id"].as_str().unwrap().to_string()
        }
    };
    let write = add("Write report", "In progress").await;
    let ship = add("Ship release", "Done").await;
    let spare = add("Spare record", "To do").await;

    // Records: permanent deletion for editors.
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":private,"user_id":carol_id,"role":"viewer"}),
    )
    .await;
    assert_eq!(
        api(
            &router,
            "DELETE",
            &format!("databases/{did}/rows/{spare}"),
            &carol,
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            &router,
            "DELETE",
            &format!("databases/{did}/rows/{spare}"),
            &admin,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        api(
            &router,
            "DELETE",
            &format!("databases/{did}/rows/{spare}"),
            &admin,
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Properties: removing an option or a property clears the values it held.
    let (_, current) = api(
        &router,
        "GET",
        &format!("databases/{did}"),
        &admin,
        json!({}),
    )
    .await;
    let mut columns = current["columns"].clone();
    columns[1]["options"] = json!(["To do", "In progress"]);
    columns
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"due","name":"Due","kind":"date","options":[]}));
    let (status, updated) = api(
        &router,
        "PATCH",
        &format!("databases/{did}"),
        &admin,
        json!({"revision":current["revision"],"columns":columns}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    let shipped = updated["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == ship.as_str())
        .unwrap();
    assert!(
        shipped["cells"].get(&status_col).is_none(),
        "the removed option is cleared"
    );
    assert_eq!(shipped["cells"][&name], "Ship release");
    api(
        &router,
        "PATCH",
        &format!("databases/{did}/rows/{write}"),
        &admin,
        json!({"revision":2,"cells":{"due":"2026-11-01"}}),
    )
    .await;
    let (_, current) = api(
        &router,
        "GET",
        &format!("databases/{did}"),
        &admin,
        json!({}),
    )
    .await;
    let mut columns = current["columns"].clone();
    columns[2]["kind"] = json!("number");
    let (_, retyped) = api(
        &router,
        "PATCH",
        &format!("databases/{did}"),
        &admin,
        json!({"revision":current["revision"],"columns":columns}),
    )
    .await;
    assert!(
        retyped["rows"][0]["cells"].get("due").is_none(),
        "values that no longer fit the type are cleared"
    );
    let columns: Vec<Value> = retyped["columns"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["id"] != "due")
        .cloned()
        .collect();
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("databases/{did}"),
        &admin,
        json!({"revision":retyped["revision"],"columns":columns}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a property with values can be deleted"
    );
    let (_, current) = api(
        &router,
        "GET",
        &format!("databases/{did}"),
        &admin,
        json!({}),
    )
    .await;
    assert_eq!(current["columns"].as_array().unwrap().len(), 2);
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("databases/{did}"),
        &admin,
        json!({"revision":current["revision"],"columns":[]}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "at least one property remains"
    );

    // Embedding: members get a live slot; code keeps the syntax literal.
    let (_, mut n) = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":notes_folder,"title":"Today"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    n["blocks"] = json!([
        {"id":"a","markdown":format!("[[database:{did}]]")},
        {"id":"b","markdown":format!("Board: [[database:{did}:board]]")},
        {"id":"c","markdown":format!("`[[database:{did}]]`")},
        {"id":"d","markdown":"[[database:not valid!]]"}
    ]);
    let (status, saved) = api(&router, "PUT", &format!("notes/{nid}"), &admin, n).await;
    assert_eq!(status, StatusCode::OK);
    let rendered = &saved["rendered"];
    assert!(
        rendered[0]
            .as_str()
            .unwrap()
            .contains(&format!("class=\"db-embed db-id-{did} db-view-table\"")),
        "{rendered}"
    );
    assert!(
        rendered[1]
            .as_str()
            .unwrap()
            .contains("Board: <span class=\"db-embed")
            && rendered[1].as_str().unwrap().contains("db-view-board")
    );
    assert!(rendered[2]
        .as_str()
        .unwrap()
        .contains(&format!("<code>[[database:{did}]]</code>")));
    assert!(rendered[3].as_str().unwrap().contains("not valid!"));
    assert!(
        !rendered[3].as_str().unwrap().contains("db-embed"),
        "malformed embeds stay text"
    );

    // Published pages never include the records.
    api(
        &router,
        "PUT",
        &format!("notes/{nid}/publication"),
        &admin,
        json!({"slug":"today","mode":"live"}),
    )
    .await;
    let (_, page) = send(&router, "GET", "/shared/today", "", Value::Null).await;
    assert!(
        page.contains("Embedded database")
            && !page.contains("Write report")
            && !page.contains("db-id-")
    );

    // Exports include a snapshot table when the exporter can read the database.
    let (_, md) = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=md&front_matter=0"),
        &admin,
        Value::Null,
    )
    .await;
    assert!(
        md.contains("**My tasks**")
            && md.contains("| Write report | In progress |")
            && md.contains("| Ship release |  |"),
        "{md}"
    );
    assert!(
        md.contains(&format!("`[[database:{did}]]`")),
        "code stays literal"
    );
    let (_, html) = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=html"),
        &admin,
        Value::Null,
    )
    .await;
    assert!(html.contains("<table>") && html.contains("Write report"));
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":private,"user_id":carol_id,"role":"none"}),
    )
    .await;
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":notes_folder,"user_id":carol_id,"role":"viewer"}),
    )
    .await;
    let (status, md) = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=md"),
        &carol,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        md.contains("not available to you") && !md.contains("Write report"),
        "{md}"
    );
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("databases/{did}"),
            &carol,
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // Databases: editors delete them; embeds then say so.
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":private,"user_id":carol_id,"role":"viewer"}),
    )
    .await;
    assert_eq!(
        api(
            &router,
            "DELETE",
            &format!("databases/{did}"),
            &carol,
            json!({})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            &router,
            "DELETE",
            &format!("databases/{did}"),
            &admin,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("databases/{did}"),
            &admin,
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, md) = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=md"),
        &admin,
        Value::Null,
    )
    .await;
    assert!(md.contains("Embedded database (deleted)"));
}
