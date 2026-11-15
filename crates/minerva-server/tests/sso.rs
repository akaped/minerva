//! Single sign-on with OpenID Connect, against a provider run by the test:
//! linking by verified email and then by subject, administrators linking from
//! Settings, accounts created for listed domains, the checks on the ID token,
//! rotated keys, SSO required, the desktop handoff, and disabled accounts.
use axum::{
    body::Body,
    extract::{Form, State},
    http::{HeaderMap, Request},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
mod common;

const CLIENT_ID: &str = "minerva";
const CLIENT_SECRET: &str = "provider-secret";
const PUBLIC: &str = "https://notes.example";

/// What the provider issues for one authorization code.
struct Grant {
    claims: Value,
    challenge: String,
}
struct Provider {
    issuer: String,
    pkcs8: Vec<u8>,
    kid: String,
    codes: HashMap<String, Grant>,
}
type Idp = Arc<Mutex<Provider>>;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn new_key() -> Vec<u8> {
    EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
        .unwrap()
        .as_ref()
        .to_vec()
}
fn key_pair(pkcs8: &[u8]) -> EcdsaKeyPair {
    EcdsaKeyPair::from_pkcs8(
        &ECDSA_P256_SHA256_FIXED_SIGNING,
        pkcs8,
        &SystemRandom::new(),
    )
    .unwrap()
}
fn sign(p: &Provider, claims: &Value) -> String {
    let header = json!({"alg": "ES256", "kid": p.kid, "typ": "JWT"});
    let input = format!(
        "{}.{}",
        B64.encode(header.to_string()),
        B64.encode(claims.to_string())
    );
    let signature = key_pair(&p.pkcs8)
        .sign(&SystemRandom::new(), input.as_bytes())
        .unwrap();
    format!("{input}.{}", B64.encode(signature.as_ref()))
}
fn challenge_of(verifier: &str) -> String {
    B64.encode(Sha256::digest(verifier.as_bytes()))
}

async fn discovery(State(idp): State<Idp>) -> Json<Value> {
    let issuer = idp.lock().unwrap().issuer.clone();
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
    }))
}
async fn jwks(State(idp): State<Idp>) -> Json<Value> {
    let p = idp.lock().unwrap();
    let public = key_pair(&p.pkcs8).public_key().as_ref().to_vec();
    Json(json!({"keys": [{
        "kty": "EC", "use": "sig", "alg": "ES256", "crv": "P-256", "kid": p.kid,
        "x": B64.encode(&public[1..33]), "y": B64.encode(&public[33..65]),
    }]}))
}
/// The token endpoint: the client's secret, the code once, and PKCE.
async fn token(
    State(idp): State<Idp>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> (axum::http::StatusCode, Json<Value>) {
    let refuse = |e: &str| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": e})),
        )
    };
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"))
    );
    if headers.get("authorization").and_then(|h| h.to_str().ok()) != Some(basic.as_str()) {
        return refuse("invalid_client");
    }
    if form.get("redirect_uri").map(String::as_str) != Some(&format!("{PUBLIC}/auth/sso/callback"))
    {
        return refuse("invalid_grant");
    }
    let mut p = idp.lock().unwrap();
    let Some(grant) = p.codes.remove(form.get("code").map_or("", String::as_str)) else {
        return refuse("invalid_grant");
    };
    if challenge_of(form.get("code_verifier").map_or("", String::as_str)) != grant.challenge {
        return refuse("invalid_grant");
    }
    let id_token = sign(&p, &grant.claims);
    (
        axum::http::StatusCode::OK,
        Json(
            json!({"access_token": "provider-access", "token_type": "Bearer", "expires_in": 300, "id_token": id_token}),
        ),
    )
}
async fn start_provider() -> Idp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let idp: Idp = Arc::new(Mutex::new(Provider {
        issuer,
        pkcs8: new_key(),
        kid: "key-1".into(),
        codes: HashMap::new(),
    }));
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .with_state(idp.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    idp
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}
impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn cookies(&self) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k == "set-cookie")
            .map(|(_, v)| v.as_str())
            .collect()
    }
    /// The session token from a `minerva_session` cookie, if one was set.
    fn session(&self) -> Option<String> {
        self.cookies().iter().find_map(|c| {
            c.strip_prefix("minerva_session=")
                .map(|rest| rest.split(';').next().unwrap().to_string())
        })
    }
}
async fn raw(r: &Router, method: &str, uri: &str, headers: &[(&str, &str)], body: String) -> Reply {
    let mut req = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let response = r
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}
async fn api(r: &Router, method: &str, path: &str, token: &str, body: Value) -> (u16, Value) {
    let auth = format!("Bearer {token}");
    let reply = raw(
        r,
        method,
        &format!("/api/v1/{path}"),
        &[
            ("content-type", "application/json"),
            ("x-minerva-client", "1"),
            ("authorization", &auth),
        ],
        body.to_string(),
    )
    .await;
    (
        reply.status,
        serde_json::from_str(&reply.body).unwrap_or(Value::Null),
    )
}
async fn email_of(r: &Router, token: &str) -> String {
    let (_, boot) = api(r, "GET", "bootstrap?notes=0", token, json!({})).await;
    boot["user"]["email"].as_str().unwrap_or("").to_string()
}

/// A sign-in sent to the provider: what `/auth/sso/start` handed the browser.
struct Started {
    state: String,
    nonce: String,
    challenge: String,
    cookie: String,
}
async fn start(r: &Router, query: &str, cookie: Option<&str>) -> Started {
    let headers: Vec<(&str, &str)> = cookie.map(|c| ("cookie", c)).into_iter().collect();
    let reply = raw(
        r,
        "GET",
        &format!("/auth/sso/start?{query}"),
        &headers,
        String::new(),
    )
    .await;
    assert_eq!(reply.status, 302, "{}", reply.body);
    let location = url::Url::parse(reply.header("location").unwrap()).unwrap();
    let q: HashMap<String, String> = location.query_pairs().into_owned().collect();
    assert_eq!(q["client_id"], CLIENT_ID);
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["redirect_uri"], format!("{PUBLIC}/auth/sso/callback"));
    let cookie = reply
        .cookies()
        .into_iter()
        .find(|c| c.starts_with("minerva_sso="))
        .unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
    Started {
        state: q["state"].clone(),
        nonce: q["nonce"].clone(),
        challenge: q["code_challenge"].clone(),
        cookie: cookie.split(';').next().unwrap().to_string(),
    }
}
/// The person signs in at the provider, which sends them back with a code.
/// `edit` changes the ID token's claims, to test the checks.
async fn finish(
    r: &Router,
    idp: &Idp,
    started: &Started,
    who: Value,
    edit: impl FnOnce(&mut Value),
) -> Reply {
    let mut claims = {
        let p = idp.lock().unwrap();
        json!({"iss": p.issuer, "aud": CLIENT_ID, "iat": now(), "exp": now() + 300, "nonce": started.nonce})
    };
    for (k, v) in who.as_object().unwrap() {
        claims[k] = v.clone();
    }
    edit(&mut claims);
    let code = uuid::Uuid::new_v4().to_string();
    idp.lock().unwrap().codes.insert(
        code.clone(),
        Grant {
            claims,
            challenge: started.challenge.clone(),
        },
    );
    raw(
        r,
        "GET",
        &format!("/auth/sso/callback?code={code}&state={}", started.state),
        &[("cookie", &started.cookie)],
        String::new(),
    )
    .await
}
async fn sign_in(r: &Router, idp: &Idp, who: Value) -> Reply {
    let started = start(r, "client=web", None).await;
    finish(r, idp, &started, who, |_| {}).await
}

#[tokio::test(flavor = "multi_thread")]
async fn single_sign_on() {
    let idp = start_provider().await;
    let issuer = idp.lock().unwrap().issuer.clone();
    // SAFETY: set before the server reads them, in this test's own process.
    unsafe {
        std::env::set_var("MINERVA_PUBLIC_URL", PUBLIC);
        std::env::set_var("MINERVA_OIDC_ISSUER", &issuer);
        std::env::set_var("MINERVA_OIDC_CLIENT_ID", CLIENT_ID);
        std::env::set_var("MINERVA_OIDC_CLIENT_SECRET", CLIENT_SECRET);
        std::env::set_var("MINERVA_OIDC_NAME", "Corp SSO");
        std::env::remove_var("MINERVA_OIDC_CREATE_DOMAINS");
        std::env::remove_var("MINERVA_OIDC_REQUIRED");
    }
    let r = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    common::account(&r, &root, "alice@corp.example").await;
    common::account(&r, &root, "bob@corp.example").await;

    // The sign-in page learns about SSO.
    let (_, health) = api(&r, "GET", "health", "", json!({})).await;
    assert_eq!(
        health["sso"],
        json!({"name": "Corp SSO", "required": false})
    );

    // A verified email links the account; later the subject alone finds it.
    let first = sign_in(&r, &idp, json!({"sub": "alice-1", "email": "Alice@corp.example", "email_verified": true, "name": "Alice"})).await;
    assert_eq!(first.status, 200, "{}", first.body);
    let token = first.session().expect("a session cookie");
    assert_eq!(email_of(&r, &token).await, "alice@corp.example");
    let (_, boot) = api(&r, "GET", "bootstrap?notes=0", &token, json!({})).await;
    assert_eq!(boot["sso_linked"], true);
    let renamed = sign_in(
        &r,
        &idp,
        json!({"sub": "alice-1", "email": "alice.new@corp.example", "email_verified": true}),
    )
    .await;
    assert_eq!(
        email_of(&r, &renamed.session().unwrap()).await,
        "alice@corp.example"
    );

    // Another identity with Alice's email cannot take her linked account.
    let other = sign_in(
        &r,
        &idp,
        json!({"sub": "mallory", "email": "alice@corp.example", "email_verified": true}),
    )
    .await;
    assert!(other.session().is_none());

    // An unverified email links nothing.
    let unverified = sign_in(
        &r,
        &idp,
        json!({"sub": "bob-1", "email": "bob@corp.example", "email_verified": false}),
    )
    .await;
    assert!(unverified.session().is_none());
    assert!(
        unverified.body.contains("no Minerva account"),
        "{}",
        unverified.body
    );

    // Administrators are never linked by email...
    let admin = sign_in(
        &r,
        &idp,
        json!({"sub": "admin-1", "email": "admin@test.local", "email_verified": true}),
    )
    .await;
    assert!(admin.session().is_none());
    assert!(admin.body.contains("not linked automatically"));
    // ...but link from Settings while signed in, which works only with the cookie.
    let anonymous = raw(&r, "GET", "/auth/sso/start?client=link", &[], String::new()).await;
    assert_ne!(anonymous.status, 302);
    let session_cookie = format!("minerva_session={root}");
    let linking = start(&r, "client=link", Some(&session_cookie)).await;
    let linked = finish(&r, &idp, &linking, json!({"sub": "admin-1"}), |_| {}).await;
    assert_eq!(linked.status, 200, "{}", linked.body);
    assert!(linked.session().is_none());
    let again = sign_in(&r, &idp, json!({"sub": "admin-1"})).await;
    assert_eq!(
        email_of(&r, &again.session().unwrap()).await,
        "admin@test.local"
    );
    // One identity links one account.
    let alice_cookie = format!("minerva_session={token}");
    let taken = start(&r, "client=link", Some(&alice_cookie)).await;
    let taken = finish(&r, &idp, &taken, json!({"sub": "admin-1"}), |_| {}).await;
    assert!(taken.body.contains("already linked"));

    // New accounts only for listed domains.
    let nobody = sign_in(
        &r,
        &idp,
        json!({"sub": "carol-1", "email": "carol@corp.example", "email_verified": true}),
    )
    .await;
    assert!(nobody.session().is_none());
    unsafe {
        std::env::set_var(
            "MINERVA_OIDC_CREATE_DOMAINS",
            "corp.example, @other.example",
        )
    };
    let carol = sign_in(&r, &idp, json!({"sub": "carol-1", "email": "carol@corp.example", "email_verified": true, "name": "Carol C."})).await;
    let carol_token = carol.session().expect("a new account");
    let (_, boot) = api(&r, "GET", "bootstrap?notes=0", &carol_token, json!({})).await;
    assert_eq!(boot["user"]["name"], "Carol C.");
    assert_eq!(boot["user"]["admin"], false);
    assert_eq!(boot["must_change_password"], false);
    let outsider = sign_in(
        &r,
        &idp,
        json!({"sub": "dave-1", "email": "dave@elsewhere.example", "email_verified": true}),
    )
    .await;
    assert!(outsider.session().is_none());
    let unverified_new = sign_in(
        &r,
        &idp,
        json!({"sub": "erin-1", "email": "erin@corp.example"}),
    )
    .await;
    assert!(unverified_new.session().is_none());
    unsafe { std::env::remove_var("MINERVA_OIDC_CREATE_DOMAINS") };

    // The ID token's checks.
    let alice = json!({"sub": "alice-1"});
    for (what, edit) in [
        (
            "nonce",
            Box::new(|c: &mut Value| c["nonce"] = json!("another")) as Box<dyn FnOnce(&mut Value)>,
        ),
        (
            "audience",
            Box::new(|c: &mut Value| c["aud"] = json!("someone-else")),
        ),
        (
            "expiry",
            Box::new(|c: &mut Value| c["exp"] = json!(now() - 600)),
        ),
        (
            "issuer",
            Box::new(|c: &mut Value| c["iss"] = json!("https://evil.example")),
        ),
    ] {
        let started = start(&r, "client=web", None).await;
        let reply = finish(&r, &idp, &started, alice.clone(), edit).await;
        assert!(reply.session().is_none(), "wrong {what} accepted");
        assert!(
            reply.body.contains("could not be verified"),
            "{what}: {}",
            reply.body
        );
    }
    // Only the browser that started a sign-in finishes it, once.
    let started = start(&r, "client=web", None).await;
    let mut elsewhere = Started {
        cookie: "minerva_sso=other".into(),
        ..started
    };
    let reply = finish(&r, &idp, &elsewhere, alice.clone(), |_| {}).await;
    assert!(reply.session().is_none());
    elsewhere.cookie = format!("minerva_sso={}", elsewhere.state);
    let replayed = finish(&r, &idp, &elsewhere, alice.clone(), |_| {}).await;
    assert!(replayed.session().is_none(), "a used state was accepted");
    let cancelled = raw(
        &r,
        "GET",
        "/auth/sso/callback?error=access_denied&state=x",
        &[],
        String::new(),
    )
    .await;
    assert!(cancelled.session().is_none());

    // The provider rotates its keys: the new key is fetched.
    {
        let mut p = idp.lock().unwrap();
        p.pkcs8 = new_key();
        p.kid = "key-2".into();
    }
    let rotated = sign_in(&r, &idp, alice.clone()).await;
    assert!(rotated.session().is_some(), "{}", rotated.body);

    // The desktop app: a one-time code for its loopback address, redeemed with
    // the verifier behind its challenge.
    let verifier = "v".repeat(43) + "-desktop-verifier";
    let query = format!(
        "client=desktop&port=50123&challenge={}",
        challenge_of(&verifier)
    );
    let desktop = start(&r, &query, None).await;
    let back = finish(&r, &idp, &desktop, alice.clone(), |_| {}).await;
    assert_eq!(back.status, 302, "{}", back.body);
    let location = url::Url::parse(back.header("location").unwrap()).unwrap();
    assert_eq!(
        location.origin().ascii_serialization(),
        "http://127.0.0.1:50123"
    );
    assert_eq!(location.path(), "/sso");
    let code = location
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned();
    let (status, session) = api(
        &r,
        "POST",
        "login/sso",
        "",
        json!({"code": code, "verifier": verifier}),
    )
    .await;
    assert_eq!(status, 200, "{session}");
    assert_eq!(
        email_of(&r, session["token"].as_str().unwrap()).await,
        "alice@corp.example"
    );
    let (status, _) = api(
        &r,
        "POST",
        "login/sso",
        "",
        json!({"code": code, "verifier": verifier}),
    )
    .await;
    assert_eq!(status, 401, "a desktop code worked twice");
    // A wrong verifier uses the code up.
    let desktop = start(&r, &query, None).await;
    let back = finish(&r, &idp, &desktop, alice.clone(), |_| {}).await;
    let location = url::Url::parse(back.header("location").unwrap()).unwrap();
    let code = location
        .query_pairs()
        .find(|(k, _)| k == "code")
        .unwrap()
        .1
        .into_owned();
    let wrong = "w".repeat(50);
    let (status, _) = api(
        &r,
        "POST",
        "login/sso",
        "",
        json!({"code": code, "verifier": wrong}),
    )
    .await;
    assert_eq!(status, 401);
    let (status, _) = api(
        &r,
        "POST",
        "login/sso",
        "",
        json!({"code": code, "verifier": verifier}),
    )
    .await;
    assert_eq!(status, 401);
    // Bad desktop requests stop before the provider.
    let bad = raw(
        &r,
        "GET",
        "/auth/sso/start?client=desktop&port=80&challenge=short",
        &[],
        String::new(),
    )
    .await;
    assert_eq!(bad.status, 200);
    assert!(bad.header("location").is_none());
    // A desktop sign-in that finds no account tells the app why.
    let desktop = start(&r, &query, None).await;
    let nobody =
        json!({"sub": "nobody", "email": "nobody@nowhere.example", "email_verified": true});
    let back = finish(&r, &idp, &desktop, nobody, |_| {}).await;
    assert_eq!(back.status, 302);
    let location = url::Url::parse(back.header("location").unwrap()).unwrap();
    assert_eq!(
        location.origin().ascii_serialization(),
        "http://127.0.0.1:50123"
    );
    let reason = location
        .query_pairs()
        .find(|(k, _)| k == "error")
        .unwrap()
        .1
        .into_owned();
    assert!(reason.contains("no Minerva account"), "{reason}");

    // SSO required: members lose their password, administrators keep theirs.
    unsafe { std::env::set_var("MINERVA_OIDC_REQUIRED", "1") };
    let (_, health) = api(&r, "GET", "health", "", json!({})).await;
    assert_eq!(health["sso"]["required"], true);
    let (status, refused) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"bob@corp.example","password":common::PASSWORD}),
    )
    .await;
    assert_eq!(status, 403, "{refused}");
    assert!(
        refused["error"].as_str().unwrap_or("").contains("Corp SSO"),
        "{refused}"
    );
    let (status, _) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"bob@corp.example","password":"wrong-password-123"}),
    )
    .await;
    assert_eq!(
        status, 401,
        "a wrong password must not learn that SSO is required"
    );
    let (status, _) = api(
        &r,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    assert_eq!(status, 200);
    unsafe { std::env::remove_var("MINERVA_OIDC_REQUIRED") };

    // Administrators see who is linked and can unlink; disabled accounts stay out.
    let (_, admin_view) = api(&r, "GET", "admin", &root, json!({})).await;
    let alice_row = admin_view["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["email"] == "alice@corp.example")
        .unwrap()
        .clone();
    assert_eq!(alice_row["sso"], true);
    let alice_id = alice_row["id"].as_str().unwrap().to_string();
    api(
        &r,
        "PATCH",
        &format!("users/{alice_id}"),
        &root,
        json!({"disabled": true}),
    )
    .await;
    let disabled = sign_in(&r, &idp, alice.clone()).await;
    assert!(disabled.session().is_none());
    assert!(disabled.body.contains("disabled"));
    api(
        &r,
        "PATCH",
        &format!("users/{alice_id}"),
        &root,
        json!({"disabled": false, "sso": null}),
    )
    .await;
    let (_, admin_view) = api(&r, "GET", "admin", &root, json!({})).await;
    let alice_row = admin_view["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["email"] == "alice@corp.example")
        .unwrap()
        .clone();
    assert_eq!(alice_row["sso"], false);
    // Unlinked, the next sign-in matches the verified email again.
    let relinked = sign_in(
        &r,
        &idp,
        json!({"sub": "alice-2", "email": "alice@corp.example", "email_verified": true}),
    )
    .await;
    let alice_token = relinked.session().expect("linked again by email");
    // People unlink themselves too.
    let (status, _) = api(&r, "DELETE", "account/sso", &alice_token, json!({})).await;
    assert_eq!(status, 200);
    let (_, boot) = api(&r, "GET", "bootstrap?notes=0", &alice_token, json!({})).await;
    assert_eq!(boot["sso_linked"], false);
}
