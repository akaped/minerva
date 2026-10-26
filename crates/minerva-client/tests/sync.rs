//! A desktop app's embedded server replicating a remote workspace: first copy,
//! offline edits on both sides, conflict copies, an unreachable server, and
//! unsent changes surviving an app restart.
use http::Method;
use minerva_client::{
    CredentialStore, Gateway, LocalServer, MemoryStore, Registry, SyncWorker, Transport,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

const PASSWORD: &str = "test-password-1234";

/// A remote server on its own runtime, so stopping it also drops open
/// connections, as a lost network would.
struct Remote {
    runtime: Option<tokio::runtime::Runtime>,
    store: minerva_server::Shared,
    address: SocketAddr,
}
impl Remote {
    fn start(store: minerva_server::Shared, address: SocketAddr) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let listener = std::net::TcpListener::bind(address).unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let app = minerva_server::app(store.clone());
        runtime.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await
        });
        Self {
            runtime: Some(runtime),
            store,
            address,
        }
    }
    fn stop(mut self) -> (minerva_server::Shared, SocketAddr) {
        self.runtime.take().unwrap().shutdown_background();
        (self.store.clone(), self.address)
    }
    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

struct SharedStore(Arc<MemoryStore>);
impl CredentialStore for SharedStore {
    fn get(&self, id: &str) -> minerva_client::Result<Option<String>> {
        self.0.get(id)
    }
    fn set(&self, id: &str, token: &str) -> minerva_client::Result<()> {
        self.0.set(id, token)
    }
    fn delete(&self, id: &str) -> minerva_client::Result<()> {
        self.0.delete(id)
    }
}

async fn remote_call(url: &str, token: &str, method: Method, path: &str, body: Value) -> Value {
    let base = url::Url::parse(&format!("{url}/")).unwrap();
    Transport::new()
        .unwrap()
        .session(base, token)
        .request(method, path, (!body.is_null()).then_some(body))
        .await
        .unwrap()
}
async fn row_value(local: &LocalServer, did: &str, rid: &str, column: &str) -> Value {
    let db = local
        .call(Method::GET, &format!("databases/{did}"), Value::Null)
        .await
        .unwrap();
    db["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == rid)
        .map(|r| r["cells"][column].clone())
        .unwrap_or(Value::Null)
}

#[tokio::test(flavor = "multi_thread")]
async fn replicas_work_offline_and_converge() {
    let store = minerva_server::Store::open(":memory:", "admin@test.local", PASSWORD).unwrap();
    let remote = Remote::start(store, "127.0.0.1:0".parse().unwrap());
    let url = remote.url();
    let base = url::Url::parse(&format!("{url}/")).unwrap();
    create_member(&url).await;
    let token = Transport::new()
        .unwrap()
        .login(&base, "member@test.local", PASSWORD)
        .await
        .unwrap()
        .token()
        .to_string();

    let credentials = Arc::new(MemoryStore::default());
    let gateway = Arc::new(Gateway::new(
        Transport::new().unwrap(),
        Registry::in_memory(),
        SharedStore(credentials.clone()),
    ));
    let connection = gateway.add(&url, "Team", false).await.unwrap();
    credentials.set(&connection.id, &token).unwrap();

    let dir = std::env::temp_dir().join(format!("minerva-replica-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("local.db");
    let local = Arc::new(LocalServer::open(&path).unwrap());

    // Personal notes exist before any server is added.
    let personal = local
        .call(Method::GET, "bootstrap", Value::Null)
        .await
        .unwrap();
    assert_eq!(personal["workspaces"][0]["name"], "My Workspace");
    let health = local
        .call(Method::GET, "health", Value::Null)
        .await
        .unwrap();
    assert_eq!(health["mode"], "local");

    // The remote workspace has a database with one record.
    let boot = remote_call(&url, &token, Method::GET, "bootstrap", Value::Null).await;
    let ws = boot["workspaces"][0]["id"].as_str().unwrap().to_string();
    let folder = boot["folders"][0]["id"].as_str().unwrap().to_string();
    let welcome = boot["notes"][0]["id"].as_str().unwrap().to_string();
    let db = remote_call(
        &url,
        &token,
        Method::POST,
        "databases",
        json!({"folder_id":folder,"name":"Tasks"}),
    )
    .await;
    let did = db["id"].as_str().unwrap().to_string();
    let column = db["columns"][0]["id"].as_str().unwrap().to_string();
    let row = remote_call(
        &url,
        &token,
        Method::POST,
        &format!("databases/{did}/rows"),
        json!({"cells":{&column:"Draft"}}),
    )
    .await;
    let rid = row["id"].as_str().unwrap().to_string();

    // Making it available offline copies everything.
    let worker = SyncWorker::new(local.clone(), gateway.clone());
    let offered = worker.available(&connection.id).await.unwrap();
    assert_eq!(offered[0]["offline"], false);
    worker.subscribe(&connection.id, &ws).await.unwrap();
    assert!(worker.run_once().await);
    let copy = local
        .call(Method::GET, "bootstrap", Value::Null)
        .await
        .unwrap();
    assert_eq!(copy["workspaces"].as_array().unwrap().len(), 2);
    let note = local
        .call(Method::GET, &format!("notes/{welcome}"), Value::Null)
        .await
        .unwrap();
    assert_eq!(note["title"], "Getting Started");
    assert_eq!(row_value(&local, &did, &rid, &column).await, "Draft");
    assert_eq!(
        worker.available(&connection.id).await.unwrap()[0]["offline"],
        true
    );

    // The server goes away. Work continues locally.
    let (store, address) = remote.stop();
    let created = local
        .call(
            Method::POST,
            "notes",
            json!({"folder_id":folder,"title":"Written offline"}),
        )
        .await
        .unwrap();
    let offline_note = created["id"].as_str().unwrap().to_string();
    local
        .call(
            Method::PATCH,
            &format!("folders/{folder}"),
            json!({"name":"Renamed offline"}),
        )
        .await
        .unwrap();
    local
        .call(
            Method::PATCH,
            &format!("databases/{did}/rows/{rid}"),
            json!({"revision":1,"cells":{&column:"Offline"}}),
        )
        .await
        .unwrap();
    assert!(!worker.run_once().await);
    let replicas = local
        .call(Method::GET, "local/replicas", Value::Null)
        .await
        .unwrap();
    assert_eq!(replicas[0]["status"]["state"], "offline");
    assert!(replicas[0]["pending"].as_i64().unwrap() >= 3);
    // Unsent changes keep the local copy from being removed by accident.
    assert!(worker.unsubscribe(&ws, false).await.is_err());

    // Meanwhile someone edits the same record on the server.
    let remote = Remote::start(store, address);
    remote_call(
        &url,
        &token,
        Method::PATCH,
        &format!("databases/{did}/rows/{rid}"),
        json!({"revision":1,"cells":{&column:"Online"}}),
    )
    .await;

    // The app restarts before syncing: the outbox is in the local database.
    drop(worker);
    drop(local);
    let local = Arc::new(LocalServer::open(&path).unwrap());
    let worker = SyncWorker::new(local.clone(), gateway.clone());
    assert!(worker.run_once().await);

    let server_boot = remote_call(&url, &token, Method::GET, "bootstrap", Value::Null).await;
    assert!(server_boot["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == offline_note.as_str() && n["title"] == "Written offline"));
    assert!(server_boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "Renamed offline"));
    // The later online edit wins; the offline value is kept as a conflict copy,
    // which also reaches the replica, and the conflict is listed locally.
    let server_db = remote_call(
        &url,
        &token,
        Method::GET,
        &format!("databases/{did}"),
        Value::Null,
    )
    .await;
    let rows = server_db["rows"].as_array().unwrap();
    assert_eq!(
        rows.iter().find(|r| r["id"] == rid.as_str()).unwrap()["cells"][&column],
        "Online"
    );
    let copy = rows
        .iter()
        .find(|r| r["conflict_of"] == rid.as_str())
        .unwrap();
    assert_eq!(copy["cells"][&column], "Offline");
    assert_eq!(row_value(&local, &did, &rid, &column).await, "Online");
    assert_eq!(
        row_value(&local, &did, copy["id"].as_str().unwrap(), &column).await,
        "Offline"
    );
    let conflicts = local
        .call(Method::GET, "local/conflicts", Value::Null)
        .await
        .unwrap();
    assert_eq!(conflicts.as_array().unwrap().len(), 1);
    assert_eq!(conflicts[0]["status"], "conflict");
    assert_eq!(conflicts[0]["entity_id"], rid.as_str());

    // Nothing is left to send, and another pass changes nothing.
    assert!(worker.run_once().await);
    let replicas = local
        .call(Method::GET, "local/replicas", Value::Null)
        .await
        .unwrap();
    assert_eq!(replicas[0]["pending"], 0, "{replicas}");
    assert_eq!(replicas[0]["status"]["state"], "synced");
    let again = remote_call(
        &url,
        &token,
        Method::GET,
        &format!("databases/{did}"),
        Value::Null,
    )
    .await;
    assert_eq!(again["rows"].as_array().unwrap().len(), 2);

    // Removing the local copy leaves the server's workspace alone.
    worker.unsubscribe(&ws, false).await.unwrap();
    let after = local
        .call(Method::GET, "bootstrap", Value::Null)
        .await
        .unwrap();
    assert_eq!(after["workspaces"].as_array().unwrap().len(), 1);
    // The member's My Workspace, the one that was copied, is still there.
    let server_boot = remote_call(&url, &token, Method::GET, "bootstrap", Value::Null).await;
    assert_eq!(server_boot["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(server_boot["workspaces"][0]["id"], ws.as_str());

    drop(remote.stop());
    std::fs::remove_dir_all(dir).unwrap();
}

/// Creates `member@test.local` with `PASSWORD` on the server at `url`: content
/// belongs to members, since administrators only manage the server.
async fn create_member(url: &str) {
    let http = reqwest::Client::new();
    let admin: Value = http
        .post(format!("{url}/api/v1/login"))
        .header("x-minerva-client", "1")
        .json(&json!({"email":"admin@test.local","password":PASSWORD}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    http.post(format!("{url}/api/v1/users"))
        .header("x-minerva-client", "1")
        .bearer_auth(admin["token"].as_str().unwrap())
        .json(&json!({"name":"Member","email":"member@test.local","password":PASSWORD}))
        .send()
        .await
        .unwrap();
}
