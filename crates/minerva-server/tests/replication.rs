//! The replication protocol a desktop replica uses against a server: change-log
//! pulls, field-clock pushes, CRDT text merging, conflict copies, idempotent
//! retries, deletes that lose to later edits, and revoked access.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;
use yrs::{updates::decoder::Decode, Array, Doc, ReadTxn, StateVector, Text, Transact, Update};
mod common;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(format!("/api/v1/{path}"))
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn login(app: &Router, email: &str) -> String {
    let (status, v) = request(
        app,
        "POST",
        "login",
        "",
        json!({"email":email,"password":"test-password-1234"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["token"].as_str().unwrap().into()
}
async fn pull(app: &Router, token: &str, ws: &str, since: i64) -> Value {
    let (status, v) = request(
        app,
        "GET",
        &format!("sync/changes?workspace={ws}&since={since}"),
        token,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v
}
async fn push(app: &Router, token: &str, ws: &str, ops: Value) -> Vec<Value> {
    let (status, v) = request(
        app,
        "POST",
        "sync/push",
        token,
        json!({"workspace_id":ws,"ops":ops}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["results"].as_array().unwrap().clone()
}
fn entry<'a>(pulled: &'a Value, kind: &str, id: &str) -> Option<&'a Value> {
    pulled["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == kind && c["id"] == id)
}
/// A clock from this replica, `ms` milliseconds after the epoch.
fn clock(ms: u64) -> String {
    format!("{ms:013}.000000.replica")
}
fn later() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    clock(ms + 60_000)
}
fn doc_from(state: &str, client: u64) -> Doc {
    let doc = Doc::with_client_id(client);
    doc.transact_mut()
        .apply_update(Update::decode_v1(&STANDARD.decode(state).unwrap()).unwrap())
        .unwrap();
    doc
}
fn encode(doc: &Doc) -> String {
    STANDARD.encode(
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default()),
    )
}

#[tokio::test]
async fn replicas_pull_push_and_merge_offline_edits() {
    let path = std::env::temp_dir().join(format!("minerva-sync-{}.db", uuid::Uuid::new_v4()));
    let server = app(Store::open(
        path.to_str().unwrap(),
        "admin@test.local",
        "test-password-1234",
    )
    .unwrap());
    let root = login(&server, "admin@test.local").await;
    let token = common::member(&server, &root, "owner@test.local").await;
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    let ws = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let nid = common::welcome(&boot)["id"].as_str().unwrap().to_string();

    // The first pull returns everything, including the welcome note's document.
    let first = pull(&server, &token, &ws, 0).await;
    assert_eq!(first["revoked"], false);
    assert!(entry(&first, "workspace", &ws).is_some());
    assert_eq!(entry(&first, "folder", &folder).unwrap()["role"], "editor");
    let note = entry(&first, "note", &nid).unwrap();
    assert!(note["crdt"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(note["clocks"]["folder_id"].as_str().is_some());
    let cursor = first["cursor"].as_i64().unwrap();
    assert_eq!(
        pull(&server, &token, &ws, cursor).await["changes"],
        json!([])
    );

    // Offline, the replica edits the welcome note while the server edits it too.
    let replica = doc_from(note["crdt"].as_str().unwrap(), 4242);
    let order = replica.get_or_insert_array("order");
    let first_block = order
        .get(&replica.transact(), 0)
        .unwrap()
        .to_string(&replica.transact());
    let text = replica.get_or_insert_text(format!("block:{first_block}"));
    text.insert(&mut replica.transact_mut(), 0, "Offline ");
    let online = doc_from(note["crdt"].as_str().unwrap(), 777);
    let online_text = online.get_or_insert_text(format!("block:{first_block}"));
    online_text.insert(&mut online.transact_mut(), 0, "Online ");
    let (status, _) = request(
        &server,
        "POST",
        &format!("notes/{nid}/sync"),
        &token,
        json!({"update":encode(&online)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // It also created a folder and a note in it while offline, with its own IDs.
    let new_folder = uuid::Uuid::new_v4().to_string();
    let new_note = uuid::Uuid::new_v4().to_string();
    let fresh = Doc::with_client_id(4243);
    {
        let title = fresh.get_or_insert_text("title");
        let order = fresh.get_or_insert_array("order");
        let block = fresh.get_or_insert_text("block:b1");
        let meta = fresh.get_or_insert_map("meta");
        let mut txn = fresh.transact_mut();
        title.insert(&mut txn, 0, "Written on a plane");
        order.insert(&mut txn, 0, "b1");
        block.insert(&mut txn, 0, "Hello from offline");
        use yrs::Map;
        meta.insert(&mut txn, "icon", "✈");
        meta.insert(&mut txn, "favorite", false);
        meta.insert(&mut txn, "trashed", false);
    }
    let c = later();
    let ops = json!([
        {"op_id":"r:1","kind":"folder","id":new_folder,"fields":{"name":"Travel","parent_id":folder},"clocks":{"name":c,"parent_id":c}},
        {"op_id":"r:2","kind":"note","id":new_note,"fields":{"folder_id":new_folder,"journal":null,"metadata":{"Author":"Me"},"print_template":""},
         "clocks":{"folder_id":c,"journal":c,"metadata":c,"print_template":c},"crdt":encode(&fresh)},
        {"op_id":"r:3","kind":"note","id":nid,"fields":{},"clocks":{},"crdt":encode(&replica)},
    ]);
    let results = push(&server, &token, &ws, ops.clone()).await;
    assert!(
        results.iter().all(|r| r["status"] == "applied"),
        "{results:?}"
    );

    let (_, merged) = request(&server, "GET", &format!("notes/{nid}"), &token, json!({})).await;
    let markdown = merged["blocks"][0]["markdown"].as_str().unwrap();
    assert!(
        markdown.contains("Offline ") && markdown.contains("Online "),
        "{markdown}"
    );
    let (_, created) = request(
        &server,
        "GET",
        &format!("notes/{new_note}"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(created["title"], "Written on a plane");
    assert_eq!(created["blocks"][0]["markdown"], "Hello from offline");
    assert_eq!(created["metadata"]["author"], "Me");

    // Retrying the same push (the response was lost) changes nothing.
    let again = push(&server, &token, &ws, ops).await;
    assert_eq!(again, results);
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    assert_eq!(
        boot["folders"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["name"] == "Travel")
            .count(),
        1
    );

    // The server normalized the metadata key, so it re-stamped that field: every
    // replica converges on the stored value.
    let after = pull(&server, &token, &ws, cursor).await;
    let pulled = entry(&after, "note", &new_note).unwrap();
    assert_eq!(pulled["fields"]["metadata"], json!({"author":"Me"}));
    assert!(pulled["clocks"]["metadata"].as_str().unwrap() > c.as_str());
    assert_eq!(pulled["clocks"]["folder_id"], c);

    // A stale rename loses to the newer value and reports it.
    let renamed = push(&server, &token, &ws, json!([
        {"op_id":"r:4","kind":"folder","id":new_folder,"fields":{"name":"Old name"},"clocks":{"name":clock(1)}}
    ])).await;
    assert_eq!(renamed[0]["status"], "merged");
    assert_eq!(renamed[0]["lost"], json!(["name"]));

    // A folder move into its own subfolder made offline lands at the top level.
    let child = uuid::Uuid::new_v4().to_string();
    let c2 = later();
    push(&server, &token, &ws, json!([
        {"op_id":"r:5","kind":"folder","id":child,"fields":{"name":"Child","parent_id":new_folder},"clocks":{"name":c2,"parent_id":c2}}
    ])).await;
    let cycle = push(&server, &token, &ws, json!([
        {"op_id":"r:6","kind":"folder","id":new_folder,"fields":{"parent_id":child},"clocks":{"parent_id":later()}}
    ])).await;
    assert_eq!(cycle[0]["status"], "merged");
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    let travel = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == new_folder.as_str())
        .unwrap();
    assert!(travel.get("parent_id").is_none(), "{travel}");

    drop(server);
    let reopened = app(Store::open(path.to_str().unwrap(), "unused", "unused").unwrap());
    let resumed = pull(&reopened, &token, &ws, cursor).await;
    assert!(entry(&resumed, "note", &new_note).is_some());
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn concurrent_record_edits_keep_a_conflict_copy() {
    let server = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&server, "admin@test.local").await;
    let token = common::member(&server, &root, "owner@test.local").await;
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    let ws = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"].as_str().unwrap();
    let (_, db) = request(
        &server,
        "POST",
        "databases",
        &token,
        json!({"folder_id":folder,"name":"Tasks"}),
    )
    .await;
    let did = db["id"].as_str().unwrap();
    let name = db["columns"][0]["id"].as_str().unwrap();
    let (_, row) = request(
        &server,
        "POST",
        &format!("databases/{did}/rows"),
        &token,
        json!({"cells":{name:"Draft"}}),
    )
    .await;
    let rid = row["id"].as_str().unwrap();
    let pulled = pull(&server, &token, &ws, 0).await;
    let record = entry(&pulled, "record", rid).unwrap();
    assert_eq!(record["fields"][format!("cells/{name}")], "Draft");

    // Online, someone edits the cell after the replica went offline and edited it
    // (the replica's clock is older).
    let (status, _) = request(
        &server,
        "PATCH",
        &format!("databases/{did}/rows/{rid}"),
        &token,
        json!({"revision":1,"cells":{name:"Online"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let field = format!("cells/{name}");
    let op = json!([{"op_id":"r:10","kind":"record","id":rid,"changed":[field],
        "fields":{field.clone():"Offline","archived":false,"database_id":did},
        "clocks":{field.clone():clock(1),"archived":clock(1),"database_id":clock(1)}}]);
    let result = push(&server, &token, &ws, op.clone()).await;
    assert_eq!(result[0]["status"], "conflict", "{result:?}");
    let copy_id = result[0]["copy_id"].as_str().unwrap();
    let (_, db) = request(
        &server,
        "GET",
        &format!("databases/{did}"),
        &token,
        json!({}),
    )
    .await;
    let rows = db["rows"].as_array().unwrap();
    let kept = rows.iter().find(|r| r["id"] == rid).unwrap();
    let copy = rows.iter().find(|r| r["id"] == copy_id).unwrap();
    assert_eq!(kept["cells"][name], "Online");
    assert_eq!(copy["cells"][name], "Offline");
    assert_eq!(copy["archived"], true);
    assert_eq!(copy["conflict_of"], rid);
    // Retried, it does not create a second copy.
    push(&server, &token, &ws, op).await;
    let (_, db) = request(
        &server,
        "GET",
        &format!("databases/{did}"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(db["rows"].as_array().unwrap().len(), 2);

    // Fields the replica did not change are merged silently, without a copy.
    let quiet = push(
        &server,
        &token,
        &ws,
        json!([{"op_id":"r:11","kind":"record","id":rid,"changed":[],
        "fields":{field.clone():"Stale"},"clocks":{field.clone():clock(2)}}]),
    )
    .await;
    assert_eq!(quiet[0]["status"], "merged");
}

#[tokio::test]
async fn deletes_lose_to_later_edits_and_revoked_members_are_told() {
    let server = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&server, "admin@test.local").await;
    let token = common::member(&server, &root, "owner@test.local").await;
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    let ws = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, note) = request(
        &server,
        "POST",
        "notes",
        &token,
        json!({"folder_id":folder,"title":"Doomed"}),
    )
    .await;
    let nid = note["id"].as_str().unwrap();

    // Deleted offline long ago, but still edited (and not trashed) online: kept.
    let kept = push(
        &server,
        &token,
        &ws,
        json!([
            {"op_id":"r:20","kind":"note","id":nid,"deleted":true,"clocks":{"deleted":clock(1000)}}
        ]),
    )
    .await;
    assert_eq!(kept[0]["status"], "conflict");
    assert_eq!(kept[0]["kept"], true);
    assert_eq!(kept[0]["entity"]["id"], nid);
    assert!(kept[0]["entity"]["crdt"].is_string());

    // Trashed online and deleted later offline: deleted, with a tombstone.
    let (_, sync) = request(
        &server,
        "POST",
        &format!("notes/{nid}/sync"),
        &token,
        json!({}),
    )
    .await;
    let doc = doc_from(sync["update"].as_str().unwrap(), 9);
    use yrs::Map;
    doc.get_or_insert_map("meta")
        .insert(&mut doc.transact_mut(), "trashed", true);
    request(
        &server,
        "POST",
        &format!("notes/{nid}/sync"),
        &token,
        json!({"update":encode(&doc)}),
    )
    .await;
    let gone = push(
        &server,
        &token,
        &ws,
        json!([
            {"op_id":"r:21","kind":"note","id":nid,"deleted":true,"clocks":{"deleted":later()}}
        ]),
    )
    .await;
    assert_eq!(gone[0]["status"], "applied", "{gone:?}");
    let pulled = pull(&server, &token, &ws, 0).await;
    let tombstone = entry(&pulled, "note", nid).unwrap();
    assert_eq!(tombstone["deleted"], true);
    assert!(tombstone["clocks"]["deleted"].is_string());

    // A member without access to the workspace is told to drop the replica, and
    // cannot push into it; only a desktop replica accepts upstream changes.
    let (_, user) = request(
        &server,
        "POST",
        "users",
        &root,
        json!({"name":"Other","email":"other@test.local","password":"test-password-1234"}),
    )
    .await;
    let other = login(&server, "other@test.local").await;
    assert_eq!(pull(&server, &other, &ws, 0).await["revoked"], true);
    let (status, _) = request(
        &server,
        "POST",
        "sync/push",
        &other,
        json!({"workspace_id":ws,"ops":[]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &server,
        "POST",
        "sync/push",
        &token,
        json!({"workspace_id":ws,"upstream":true,"ops":[]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Granting access changes the access digest, which makes replicas resync.
    let before = pull(&server, &token, &ws, 0).await["access"].clone();
    request(
        &server,
        "PUT",
        "workspace-access",
        &token,
        json!({"workspace_id":ws,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let shared = pull(&server, &other, &ws, 0).await;
    assert_eq!(shared["revoked"], false);
    assert_eq!(entry(&shared, "folder", &folder).unwrap()["role"], "viewer");
    assert_ne!(shared["access"], before);
    // IDs chosen by replicas end up in markup and URLs: only UUIDs are accepted.
    let odd = push(&server, &token, &ws, json!([
        {"op_id":"r:30","kind":"folder","id":"\"><img src=x>","fields":{"name":"X","parent_id":null},"clocks":{"name":later(),"parent_id":later()}}
    ])).await;
    assert_eq!(odd[0]["status"], "invalid");
    // A workspace's journal cannot point into another workspace.
    let (_, other_ws) = request(
        &server,
        "POST",
        "workspaces",
        &token,
        json!({"name":"Elsewhere"}),
    )
    .await;
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    let foreign = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] == other_ws["id"])
        .unwrap()["id"]
        .clone();
    push(&server, &token, &ws, json!([
        {"op_id":"r:31","kind":"workspace","id":ws,"fields":{"journal_folder":foreign},"clocks":{"journal_folder":later()}}
    ])).await;
    let (_, boot) = request(&server, "GET", "bootstrap", &token, json!({})).await;
    assert!(
        boot["journal_folders"].get(&ws).is_none(),
        "{}",
        boot["journal_folders"]
    );
    let denied = push(&server, &other, &ws, json!([
        {"op_id":"o:1","kind":"folder","id":folder,"fields":{"name":"Mine"},"clocks":{"name":later()}}
    ])).await;
    assert_eq!(denied[0]["status"], "forbidden");
}

#[tokio::test]
async fn a_replica_is_read_only_where_the_server_grants_view_access() {
    let (store, token) = Store::open_local(":memory:").unwrap();
    let local = app(store);
    let (status, health) = request(&local, "GET", "health", "", json!({})).await;
    assert_eq!(
        (status, health["mode"].as_str()),
        (StatusCode::OK, Some("local"))
    );
    // The local owner never signs in with a password.
    let (status, _) = request(
        &local,
        "POST",
        "login",
        "",
        json!({"email":"local@minerva.localhost","password":"x"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let ws = uuid::Uuid::new_v4().to_string();
    let folder = uuid::Uuid::new_v4().to_string();
    let (status, _) = request(
        &local,
        "POST",
        "local/replicas",
        &token,
        json!({"workspace_id":ws,"connection_id":"c1","server_id":"remote","name":"Team"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let c = later();
    let (status, applied) = request(&local, "POST", "sync/push", &token, json!({"workspace_id":ws,"upstream":true,"cursor":7,"access":"digest","ops":[
        {"op_id":"remote:1","kind":"workspace","id":ws,"role":"viewer","fields":{"name":"Team"},"clocks":{"name":c}},
        {"op_id":"remote:2","kind":"folder","id":folder,"role":"viewer","fields":{"name":"Docs","parent_id":null},"clocks":{"name":c,"parent_id":c}},
    ]})).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    let (_, replicas) = request(&local, "GET", "local/replicas", &token, json!({})).await;
    assert_eq!(replicas[0]["pull_cursor"], 7);
    assert_eq!(replicas[0]["owner"], false);
    assert_eq!(
        replicas[0]["pending"], 0,
        "applied upstream changes are not sent back"
    );

    let (_, boot) = request(&local, "GET", "bootstrap", &token, json!({})).await;
    assert_eq!(boot["roles"][&folder], "viewer");
    let (status, _) = request(
        &local,
        "POST",
        "notes",
        &token,
        json!({"folder_id":folder,"title":"Nope"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &local,
        "PATCH",
        &format!("folders/{folder}"),
        &token,
        json!({"name":"Mine"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Personal workspaces stay fully editable.
    let personal = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] != folder.as_str())
        .unwrap()["id"]
        .clone();
    let (status, _) = request(
        &local,
        "POST",
        "notes",
        &token,
        json!({"folder_id":personal,"title":"Mine"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
