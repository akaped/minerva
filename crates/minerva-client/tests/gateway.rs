use http::{Request, StatusCode};
use minerva_client::{
    Connection, CredentialStore, Error, Gateway, MemoryStore, Registry, Transport,
};
use serde_json::{json, Value};
use std::sync::Arc;

const PASSWORD: &str = "test-password-1234";
const ORIGIN: &str = "minerva://stest";

async fn server() -> String {
    let store = minerva_server::Store::open(":memory:", "admin@test.local", PASSWORD).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, minerva_server::app(store)).await });
    let url = format!("http://{address}");
    create_member(&url).await;
    url
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

fn request(method: &str, path: &str, body: Value) -> Request<Vec<u8>> {
    Request::builder()
        .method(method)
        .uri(format!("{ORIGIN}{path}"))
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("origin", ORIGIN)
        .body(if body.is_null() {
            vec![]
        } else {
            body.to_string().into_bytes()
        })
        .unwrap()
}
fn json_body(response: &http::Response<Vec<u8>>) -> Value {
    serde_json::from_slice(response.body()).unwrap()
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

#[tokio::test]
async fn gateway_authenticates_with_stored_tokens_and_isolates_origins() {
    let url = server().await;
    let credentials = Arc::new(MemoryStore::default());
    let gateway = Gateway::new(
        Transport::new().unwrap(),
        Registry::in_memory(),
        SharedStore(credentials.clone()),
    );
    let connection = gateway.add(&url, "Local", false).await.unwrap();
    assert_eq!(connection.url, format!("{url}/"));
    assert_eq!(
        gateway.add(&url, "Again", false).await.unwrap().id,
        connection.id
    );
    let id = connection.id.as_str();

    let page = gateway
        .handle(id, ORIGIN, request("GET", "/", Value::Null))
        .await;
    assert_eq!(page.status(), StatusCode::OK);
    let html = String::from_utf8_lossy(page.body());
    assert!(html.contains("/app.js"));
    assert!(html.contains(&format!(
        "<meta name=\"minerva-server-origin\" content=\"{url}\" />"
    )));
    // Note tabs too: their share dialog shows the server's public addresses.
    let pane = gateway
        .handle(id, ORIGIN, request("GET", "/pane", Value::Null))
        .await;
    assert!(String::from_utf8_lossy(pane.body()).contains(&format!(
        "<meta name=\"minerva-server-origin\" content=\"{url}\" />"
    )));
    let unauthenticated = gateway
        .handle(id, ORIGIN, request("GET", "/api/v1/bootstrap", Value::Null))
        .await;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let login = gateway
        .handle(
            id,
            ORIGIN,
            request(
                "POST",
                "/api/v1/login",
                json!({"email":"member@test.local","password":PASSWORD}),
            ),
        )
        .await;
    assert_eq!(login.status(), StatusCode::OK);
    assert!(
        login.headers().get("set-cookie").is_none(),
        "cookies must not reach the webview"
    );
    let login_body = json_body(&login);
    assert!(
        login_body.get("token").is_none(),
        "the page must not receive the token"
    );
    assert_eq!(login_body["user"]["email"], "member@test.local");
    let token = credentials
        .get(id)
        .unwrap()
        .expect("token stored in credential store");
    assert!(gateway.signed_in(id));
    assert_eq!(
        gateway.connection(id).unwrap().account.as_deref(),
        Some("member@test.local")
    );

    let boot = gateway
        .handle(id, ORIGIN, request("GET", "/api/v1/bootstrap", Value::Null))
        .await;
    assert_eq!(boot.status(), StatusCode::OK);
    let boot = json_body(&boot);
    let note_id = boot["notes"][0]["id"].as_str().unwrap().to_string();

    // Another connection's page, or any foreign page, cannot borrow this session.
    let mut foreign = request("GET", "/api/v1/bootstrap", Value::Null);
    foreign
        .headers_mut()
        .insert("referer", "minerva://sother/".parse().unwrap());
    foreign.headers_mut().remove("origin");
    assert_eq!(
        gateway.handle(id, ORIGIN, foreign).await.status(),
        StatusCode::FORBIDDEN
    );
    let mut foreign = request("POST", "/api/v1/search", json!({"query":"Minerva"}));
    foreign
        .headers_mut()
        .insert("origin", "minerva://app".parse().unwrap());
    assert_eq!(
        gateway.handle(id, ORIGIN, foreign).await.status(),
        StatusCode::FORBIDDEN
    );

    // Binary uploads larger than one read buffer survive the round trip intact.
    let bytes: Vec<u8> = (0..1_500_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let upload = Request::builder()
        .method("POST")
        .uri(format!(
            "{ORIGIN}/api/v1/notes/{note_id}/attachments?filename=data%20file.bin"
        ))
        .header("x-minerva-client", "1")
        .header("origin", ORIGIN)
        .body(bytes.clone())
        .unwrap();
    let upload = gateway.handle(id, ORIGIN, upload).await;
    assert_eq!(
        upload.status(),
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(upload.body())
    );
    let attachment = json_body(&upload);
    let download = gateway
        .handle(
            id,
            ORIGIN,
            request(
                "GET",
                &format!("/api/v1/attachments/{}", attachment["id"].as_str().unwrap()),
                Value::Null,
            ),
        )
        .await;
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(download.body(), &bytes);
    assert!(download.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .starts_with("attachment"));

    // The typed transport shares the same token and API contract.
    let base = minerva_client::normalize_server_url(&url, false).unwrap();
    let session = Transport::new().unwrap().session(base, token);
    let note = session.note(&note_id).await.unwrap();
    let mut save = minerva_core::SaveNote {
        title: "Renamed natively".into(),
        icon: note.icon.clone(),
        blocks: note.blocks.clone(),
        revision: note.revision,
        favorite: note.favorite,
        trashed: false,
        cover: note.cover.clone(),
        tags: note.tags.clone(),
    };
    assert_eq!(
        session.save_note(&note_id, &save).await.unwrap().title,
        "Renamed natively"
    );
    save.title = "Stale".into();
    assert!(matches!(
        session.save_note(&note_id, &save).await,
        Err(Error::Api(409, _))
    ));
    assert_eq!(
        session
            .search("Renamed")
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let logout = gateway
        .handle(id, ORIGIN, request("POST", "/api/v1/logout", Value::Null))
        .await;
    assert_eq!(logout.status(), StatusCode::OK);
    assert!(credentials.get(id).unwrap().is_none());
    assert!(
        matches!(session.bootstrap().await, Err(Error::Api(401, _))),
        "logout revokes on the server"
    );
    let after = gateway
        .handle(id, ORIGIN, request("GET", "/api/v1/bootstrap", Value::Null))
        .await;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn changed_server_identity_blocks_tokens_until_trusted() {
    let url = server().await;
    let credentials = Arc::new(MemoryStore::default());
    credentials
        .set("c1", "token-for-a-different-server")
        .unwrap();
    let mut registry = Registry::in_memory();
    registry
        .upsert(Connection {
            id: "c1".into(),
            label: "Moved".into(),
            url: format!("{url}/"),
            server_id: "previous-server-id".into(),
            account: Some("admin@test.local".into()),
            allow_insecure_http: false,
        })
        .unwrap();
    let gateway = Gateway::new(
        Transport::new().unwrap(),
        registry,
        SharedStore(credentials.clone()),
    );

    let blocked = gateway
        .handle(
            "c1",
            ORIGIN,
            request("GET", "/api/v1/bootstrap", Value::Null),
        )
        .await;
    assert_eq!(blocked.status(), StatusCode::MISDIRECTED_REQUEST);
    assert!(matches!(
        gateway.verify("c1").await,
        Err(Error::IdentityChanged { .. })
    ));

    gateway.trust_current_identity("c1").await.unwrap();
    assert!(
        credentials.get("c1").unwrap().is_none(),
        "old server's token is discarded"
    );
    assert_ne!(
        gateway.connection("c1").unwrap().server_id,
        "previous-server-id"
    );
    let fresh = gateway
        .handle(
            "c1",
            ORIGIN,
            request("GET", "/api/v1/bootstrap", Value::Null),
        )
        .await;
    assert_eq!(fresh.status(), StatusCode::UNAUTHORIZED);

    gateway.remove("c1").await.unwrap();
    assert!(gateway.connections().is_empty());
}

#[test]
fn registry_persists_without_secrets() {
    let dir = std::env::temp_dir().join(format!("minerva-registry-{}", uuid_like()));
    let path = dir.join("connections.json");
    let mut registry = Registry::load(&path).unwrap();
    registry
        .upsert(Connection {
            id: "c1".into(),
            label: "Team".into(),
            url: "https://notes.example.com/".into(),
            server_id: "sid".into(),
            account: None,
            allow_insecure_http: false,
        })
        .unwrap();
    let reloaded = Registry::load(&path).unwrap();
    assert_eq!(reloaded.connections(), registry.connections());
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("token"));
    std::fs::remove_dir_all(dir).unwrap();
}
fn uuid_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// Uses the real OS credential store (Secret Service / Credential Manager).
/// Run with `cargo test -p minerva-client -- --ignored` on an unlocked desktop session.
#[test]
#[ignore]
fn system_keyring_round_trip() {
    let store = minerva_client::KeyringStore::new("minerva-desktop-test");
    store.set("round-trip", "secret-token").unwrap();
    assert_eq!(
        store.get("round-trip").unwrap().as_deref(),
        Some("secret-token")
    );
    store.delete("round-trip").unwrap();
    assert_eq!(store.get("round-trip").unwrap(), None);
    store.delete("round-trip").unwrap();
}

/// The current authenticator code for a Base32 secret.
fn totp(secret: &str, offset: u64) -> String {
    use hmac::{Hmac, Mac};
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut key, mut buffer, mut bits) = (vec![], 0u32, 0);
    for c in secret.chars().filter(|c| !c.is_whitespace()) {
        buffer = (buffer << 5) | alphabet.iter().position(|&a| a as char == c).unwrap() as u32;
        bits += 5;
        if bits >= 8 {
            key.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(&key).unwrap();
    mac.update(&(now / 30 + offset).to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let o = (digest[19] & 0x0f) as usize;
    let value = u32::from_be_bytes([
        digest[o] & 0x7f,
        digest[o + 1],
        digest[o + 2],
        digest[o + 3],
    ]) % 1_000_000;
    format!("{value:06}")
}

#[tokio::test]
async fn two_factor_sign_in_through_the_gateway_and_typed_client() {
    let url = server().await;
    let base = minerva_client::normalize_server_url(&url, false).unwrap();
    let transport = Transport::new().unwrap();
    let session = transport
        .login(&base, "admin@test.local", PASSWORD)
        .await
        .unwrap();
    // Turn on 2FA with the typed session's token.
    let http = reqwest::Client::new();
    let setup: Value = http
        .post(format!("{url}/api/v1/account/2fa/setup"))
        .bearer_auth(session.token())
        .header("x-minerva-client", "1")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let secret = setup["secret"].as_str().unwrap().replace(' ', "");
    let enabled = http
        .post(format!("{url}/api/v1/account/2fa/enable"))
        .bearer_auth(session.token())
        .header("x-minerva-client", "1")
        .json(&json!({"code": totp(&secret, 0)}))
        .send()
        .await
        .unwrap();
    assert!(enabled.status().is_success());
    let enabled: Value = enabled.json().await.unwrap();
    let recovery = enabled["recovery_codes"][0].as_str().unwrap().to_string();

    // Typed client: a code is required, then accepted (a recovery code here, since
    // each authenticator code works only once).
    assert!(matches!(
        transport.login(&base, "admin@test.local", PASSWORD).await,
        Err(Error::Invalid(_))
    ));
    let typed = transport
        .login_with_code(&base, "admin@test.local", PASSWORD, Some(&recovery))
        .await
        .unwrap();
    assert!(typed.bootstrap().await.is_ok());

    // Gateway: the first step stores nothing; the second step's token is captured.
    let credentials = Arc::new(MemoryStore::default());
    let gateway = Gateway::new(
        Transport::new().unwrap(),
        Registry::in_memory(),
        SharedStore(credentials.clone()),
    );
    let id = gateway.add(&url, "Local", false).await.unwrap().id;
    let first = gateway
        .handle(
            &id,
            ORIGIN,
            request(
                "POST",
                "/api/v1/login",
                json!({"email":"admin@test.local","password":PASSWORD}),
            ),
        )
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first = json_body(&first);
    assert_eq!(first["two_factor_required"], true);
    assert!(credentials.get(&id).unwrap().is_none());
    let second = gateway
        .handle(
            &id,
            ORIGIN,
            request(
                "POST",
                "/api/v1/login/2fa",
                json!({"challenge":first["challenge"],"code": totp(&secret, 1)}),
            ),
        )
        .await;
    assert_eq!(
        second.status(),
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(second.body())
    );
    assert!(
        json_body(&second).get("token").is_none(),
        "the page never receives the token"
    );
    assert!(second.headers().get("set-cookie").is_none());
    assert!(credentials.get(&id).unwrap().is_some());
    assert_eq!(
        gateway
            .handle(
                &id,
                ORIGIN,
                request("GET", "/api/v1/bootstrap", Value::Null)
            )
            .await
            .status(),
        StatusCode::OK
    );
}
