//! Two-factor authentication with an authenticator app (TOTP).
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use hmac::{Hmac, Mac};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;

const PASSWORD: &str = "test-password-1234";

async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
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
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
/// What an authenticator app shows for a Base32 secret at `offset` steps from now.
fn totp(secret: &str, offset: i64) -> String {
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
        .as_secs() as i64;
    let counter = (now / 30 + offset) as u64;
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(&key).unwrap();
    mac.update(&counter.to_be_bytes());
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

/// A server with an admin and Ana, who has 2FA enabled. Returns
/// (router, admin token, Ana's session, secret, recovery codes).
async fn enabled() -> (Router, String, String, String, Vec<String>) {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    let admin = login["token"].as_str().unwrap().to_string();
    api(
        &router,
        "POST",
        "users",
        &admin,
        json!({"name":"Ana","email":"ana@test.local","password":PASSWORD}),
    )
    .await;
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"ana@test.local","password":PASSWORD}),
    )
    .await;
    let ana = login["token"].as_str().unwrap().to_string();
    let setup = api(&router, "POST", "account/2fa/setup", &ana, json!({}))
        .await
        .1;
    let secret = setup["secret"].as_str().unwrap().replace(' ', "");
    let enabled = api(
        &router,
        "POST",
        "account/2fa/enable",
        &ana,
        json!({"code": totp(&secret, 0)}),
    )
    .await
    .1;
    let recovery = enabled["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    (router, admin, ana, secret, recovery)
}
async fn challenge(router: &Router) -> String {
    let (status, first) = api(
        router,
        "POST",
        "login",
        "",
        json!({"email":"ana@test.local","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    first["challenge"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn setup_requires_a_valid_code_and_sign_in_needs_the_second_step() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    let admin = login["token"].as_str().unwrap().to_string();
    api(
        &router,
        "POST",
        "users",
        &admin,
        json!({"name":"Ana","email":"ana@test.local","password":PASSWORD}),
    )
    .await;
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"ana@test.local","password":PASSWORD}),
    )
    .await;
    let ana = login["token"].as_str().unwrap().to_string();
    let (_, second) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"ana@test.local","password":PASSWORD}),
    )
    .await;
    let other_session = second["token"].as_str().unwrap().to_string();

    assert_eq!(
        api(&router, "GET", "account/2fa", &ana, json!({})).await.1["enabled"],
        false
    );
    assert_eq!(
        api(
            &router,
            "POST",
            "account/2fa/enable",
            &ana,
            json!({"code":"123456"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST,
        "no setup started"
    );
    let (status, setup) = api(&router, "POST", "account/2fa/setup", &ana, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let secret = setup["secret"].as_str().unwrap().replace(' ', "");
    assert_eq!(secret.len(), 32, "160-bit secret");
    let uri = setup["uri"].as_str().unwrap();
    assert!(
        uri.starts_with("otpauth://totp/Minerva%3Aana%40test.local?secret=")
            && uri.contains("issuer=Minerva")
            && uri.contains("digits=6&period=30"),
        "{uri}"
    );
    assert!(setup["qr_svg"].as_str().unwrap().contains("<svg"));
    assert_eq!(
        api(
            &router,
            "POST",
            "account/2fa/enable",
            &ana,
            json!({"code":"000000"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, enabled) = api(
        &router,
        "POST",
        "account/2fa/enable",
        &ana,
        json!({"code": totp(&secret, 0)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{enabled}");
    assert_eq!(enabled["recovery_codes"].as_array().unwrap().len(), 10);
    // The session that enabled 2FA stays; other sessions are signed out.
    assert_eq!(
        api(&router, "GET", "bootstrap", &ana, json!({})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        api(&router, "GET", "bootstrap", &other_session, json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(&router, "POST", "account/2fa/setup", &ana, json!({}))
            .await
            .0,
        StatusCode::BAD_REQUEST,
        "already on"
    );

    // The password alone gives no session.
    let (status, first) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"ana@test.local","password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["two_factor_required"], true);
    assert!(first.get("token").is_none());
    let challenge = first["challenge"].as_str().unwrap().to_string();
    assert_eq!(
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":challenge,"code":"000000"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    // The code used to enable 2FA cannot be replayed; the next step's code works.
    assert_eq!(
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":challenge,"code": totp(&secret, 0)})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, signed_in) = api(
        &router,
        "POST",
        "login/2fa",
        "",
        json!({"challenge":challenge,"code": totp(&secret, 1)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{signed_in}");
    let session = signed_in["token"].as_str().unwrap().to_string();
    assert_eq!(
        api(&router, "GET", "bootstrap", &session, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":challenge,"code": totp(&secret, 1)})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED,
        "challenges are single use"
    );
    assert_eq!(
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":"forged","code": totp(&secret, -1)})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn wrong_codes_end_the_challenge_and_recovery_codes_are_single_use() {
    let (router, _, session, secret, recovery) = enabled().await;
    let id = challenge(&router).await;
    for _ in 0..5 {
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":id,"code":"111111"}),
        )
        .await;
    }
    let (_, ended) = api(
        &router,
        "POST",
        "login/2fa",
        "",
        json!({"challenge":id,"code": recovery[0]}),
    )
    .await;
    assert!(
        ended["error"].as_str().unwrap().contains("password again"),
        "{ended}"
    );

    // A recovery code works once, in any case or spacing.
    let id = challenge(&router).await;
    assert_eq!(
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":id,"code": recovery[0].to_uppercase().replace('-', " ")})
        )
        .await
        .0,
        StatusCode::OK
    );
    let id = challenge(&router).await;
    assert_eq!(
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":id,"code": recovery[0]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(&router, "GET", "account/2fa", &session, json!({}))
            .await
            .1["recovery_remaining"],
        9
    );

    // New recovery codes need the app, not a recovery code.
    assert_eq!(
        api(
            &router,
            "POST",
            "account/2fa/recovery",
            &session,
            json!({"code": recovery[1]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, fresh) = api(
        &router,
        "POST",
        "account/2fa/recovery",
        &session,
        json!({"code": totp(&secret, 1)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{fresh}");
    assert_eq!(
        api(&router, "GET", "account/2fa", &session, json!({}))
            .await
            .1["recovery_remaining"],
        10
    );
    let id = challenge(&router).await;
    assert_eq!(
        api(
            &router,
            "POST",
            "login/2fa",
            "",
            json!({"challenge":id,"code": recovery[1]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED,
        "old codes are replaced"
    );
}

#[tokio::test]
async fn disabling_needs_password_and_code_and_admins_can_reset() {
    let (router, admin, session, _, recovery) = enabled().await;
    assert_eq!(
        api(
            &router,
            "POST",
            "account/2fa/disable",
            &session,
            json!({"password":"wrong-password-x","code": recovery[0]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(
            &router,
            "POST",
            "account/2fa/disable",
            &session,
            json!({"password":PASSWORD,"code":"000000"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(
            &router,
            "POST",
            "account/2fa/disable",
            &session,
            json!({"password":PASSWORD,"code": recovery[0]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let plain = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"ana@test.local","password":PASSWORD}),
    )
    .await
    .1;
    assert!(plain["token"].is_string(), "password-only sign-in again");

    // Administrators see who uses 2FA and can reset it for a lost device.
    let setup = api(&router, "POST", "account/2fa/setup", &session, json!({}))
        .await
        .1;
    let secret = setup["secret"].as_str().unwrap().replace(' ', "");
    api(
        &router,
        "POST",
        "account/2fa/enable",
        &session,
        json!({"code": totp(&secret, 0)}),
    )
    .await;
    let listing = api(&router, "GET", "admin", &admin, json!({})).await.1;
    let entry = listing["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["email"] == "ana@test.local")
        .unwrap()
        .clone();
    assert_eq!(entry["two_factor"], true);
    api(
        &router,
        "PATCH",
        &format!("users/{}", entry["id"].as_str().unwrap()),
        &admin,
        json!({"two_factor":false}),
    )
    .await;
    let plain = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"ana@test.local","password":PASSWORD}),
    )
    .await
    .1;
    assert!(plain["token"].is_string(), "reset removes the second step");
}
