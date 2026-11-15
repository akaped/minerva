//! Production hardening: sign-in limits by client address, limits on password
//! checks by signed-in accounts, who deletes folders and manages access, and
//! which records a synced note may claim as its page.
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;
use yrs::{Doc, ReadTxn, StateVector, Transact};
mod common;
use common::PASSWORD;

/// A request from `from` (a client address, as the server's listener records
/// it), or from an unknown address.
async fn call(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
    from: Option<&str>,
    spoofed: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(format!("/api/v1/{path}"))
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"));
    if let Some(value) = spoofed {
        request = request.header("x-minerva-peer", value);
    }
    let mut request = request.body(Body::from(body.to_string())).unwrap();
    if let Some(ip) = from {
        request.extensions_mut().insert(ConnectInfo::<SocketAddr>(
            format!("{ip}:40000").parse().unwrap(),
        ));
    }
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    call(router, method, path, token, body, None, None).await
}
async fn login(router: &Router, email: &str, password: &str, from: Option<&str>) -> StatusCode {
    call(
        router,
        "POST",
        "login",
        "",
        json!({"email":email,"password":password}),
        from,
        None,
    )
    .await
    .0
}
async fn server() -> (Router, String) {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (_, v) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    (router, v["token"].as_str().unwrap().to_string())
}
async fn user_id(router: &Router, token: &str) -> String {
    let (_, boot) = api(router, "GET", "bootstrap", token, json!({})).await;
    boot["user"]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn one_address_cannot_lock_everyone_out_of_signing_in() {
    let (router, root) = server().await;
    common::account(&router, &root, "victim@test.local").await;
    common::account(&router, &root, "other@test.local").await;

    // An attacker sprays wrong passwords at made-up accounts from one address.
    for i in 0..30 {
        let status = login(
            &router,
            &format!("nobody{i}@test.local"),
            "wrong-password-1",
            Some("8.8.8.8"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {i}");
    }
    assert_eq!(
        login(
            &router,
            "nobody99@test.local",
            "wrong-password-1",
            Some("8.8.8.8")
        )
        .await,
        StatusCode::TOO_MANY_REQUESTS,
        "that address is now throttled"
    );
    // A copy of the server's own address header sent by the client is ignored.
    let (status, _) = call(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"x@test.local","password":"wrong-password-1"}),
        Some("8.8.8.8"),
        Some("4.4.4.4"),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // Everyone else still signs in, and so do people with no known address.
    assert_eq!(
        login(&router, "other@test.local", PASSWORD, Some("1.1.1.1")).await,
        StatusCode::OK
    );
    assert_eq!(
        login(&router, "other@test.local", PASSWORD, None).await,
        StatusCode::OK
    );

    // Wrong passwords for one account from one address throttle that pair only:
    // its owner still signs in from elsewhere.
    for _ in 0..10 {
        assert_eq!(
            login(
                &router,
                "victim@test.local",
                "wrong-password-1",
                Some("9.9.9.9")
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        login(&router, "victim@test.local", PASSWORD, Some("9.9.9.9")).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        login(&router, "victim@test.local", PASSWORD, Some("1.1.1.1")).await,
        StatusCode::OK
    );

    // Sign-ins that succeed do not count.
    for i in 0..15 {
        assert_eq!(
            login(&router, "other@test.local", PASSWORD, Some("1.0.0.1")).await,
            StatusCode::OK,
            "sign-in {i}"
        );
    }
}

#[tokio::test]
async fn a_signed_in_session_cannot_guess_the_password_without_limit() {
    let (router, root) = server().await;
    let token = common::account(&router, &root, "member@test.local").await;
    for _ in 0..10 {
        let (status, _) = api(
            &router,
            "POST",
            "account/2fa/disable",
            &token,
            json!({"password":"wrong-password-1","code":"000000"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, _) = api(
        &router,
        "POST",
        "account/2fa/disable",
        &token,
        json!({"password":PASSWORD,"code":"000000"}),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // The same budget covers changing the password.
    let (status, _) = api(
        &router,
        "POST",
        "account/password",
        &token,
        json!({"current_password":PASSWORD,"new_password":"another-password-1"}),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn only_workspace_editors_delete_folders_and_only_owners_manage_access() {
    let (router, root) = server().await;
    let owner = common::member(&router, &root, "owner@test.local").await;
    let folder_editor = common::account(&router, &root, "folder@test.local").await;
    let workspace_editor = common::account(&router, &root, "editor@test.local").await;
    let outsider = common::account(&router, &root, "outsider@test.local").await;
    let (folder_id, editor_id, outsider_id) = (
        user_id(&router, &folder_editor).await,
        user_id(&router, &workspace_editor).await,
        user_id(&router, &outsider).await,
    );
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let ws = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, kept) = api(
        &router,
        "POST",
        "folders",
        &owner,
        json!({"workspace_id":ws,"name":"Kept"}),
    )
    .await;
    let (_, doomed) = api(
        &router,
        "POST",
        "folders",
        &owner,
        json!({"workspace_id":ws,"name":"Doomed"}),
    )
    .await;
    let (kept, doomed) = (kept["id"].as_str().unwrap(), doomed["id"].as_str().unwrap());
    let (status, _) = api(
        &router,
        "PUT",
        "permissions",
        &owner,
        json!({"folder_id":kept,"user_id":folder_id,"role":"editor"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = api(
        &router,
        "PUT",
        "workspace-access",
        &owner,
        json!({"workspace_id":ws,"user_id":editor_id,"role":"editor"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Editing one folder is not enough to destroy it.
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("folders/{kept}"),
        &folder_editor,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("folders/{doomed}"),
        &workspace_editor,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Editors of the workspace do not choose who has access; the owner does.
    for (path, body) in [
        (
            "workspace-access",
            json!({"workspace_id":ws,"user_id":outsider_id,"role":"viewer"}),
        ),
        (
            "workspace-access",
            json!({"workspace_id":ws,"user_id":folder_id,"role":"none"}),
        ),
        (
            "permissions",
            json!({"folder_id":kept,"user_id":outsider_id,"role":"editor"}),
        ),
        (
            "permissions",
            json!({"folder_id":kept,"user_id":folder_id,"role":"none"}),
        ),
    ] {
        let (status, _) = api(&router, "PUT", path, &workspace_editor, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    let members = format!("workspaces/{ws}/members");
    let (_, seen) = api(&router, "GET", &members, &workspace_editor, json!({})).await;
    assert_eq!(seen["can_manage"], false);
    assert_eq!(seen["can_edit"], true);
    // Only the people who have access, not everyone on the server.
    assert!(
        seen["members"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["id"] != outsider_id.as_str()),
        "{seen}"
    );
    let (_, managed) = api(&router, "GET", &members, &owner, json!({})).await;
    assert_eq!(managed["can_manage"], true);
    // Even the owner is not shown the server's other accounts...
    assert!(
        managed["members"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["id"] != outsider_id.as_str()),
        "{managed}"
    );
    // ...but invites them by their exact email.
    let (status, found) = api(
        &router,
        "PUT",
        "workspace-access",
        &owner,
        json!({"workspace_id":ws,"email":"nobody@test.local","role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{found}");
    let (status, _) = api(
        &router,
        "PUT",
        "workspace-access",
        &owner,
        json!({"workspace_id":ws,"email":" Outsider@Test.local ","role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, managed) = api(&router, "GET", &members, &owner, json!({})).await;
    assert!(
        managed["members"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == outsider_id.as_str() && m["role"] == "viewer"),
        "{managed}"
    );
}

#[tokio::test]
async fn nobody_lists_the_accounts_on_the_server() {
    let (router, root) = server().await;
    let alice = common::member(&router, &root, "alice@test.local").await;
    let bob = common::account(&router, &root, "bob@test.local").await;
    let carol = common::account(&router, &root, "carol@test.local").await;
    let bob_id = user_id(&router, &bob).await;
    let carol_id = user_id(&router, &carol).await;
    let (_, boot) = api(&router, "GET", "bootstrap", &alice, json!({})).await;
    let team = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, created) = api(
        &router,
        "POST",
        "workspaces",
        &alice,
        json!({"name":"Second"}),
    )
    .await;
    let second = created["id"].as_str().unwrap().to_string();
    let listed = |v: &Value| -> Vec<String> {
        v["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect()
    };
    // Bob's own workspaces show nobody else, though he may create workspaces.
    let (_, bob_boot) = api(&router, "GET", "bootstrap", &bob, json!({})).await;
    for w in bob_boot["workspaces"].as_array().unwrap() {
        let path = format!("workspaces/{}/members", w["id"].as_str().unwrap());
        let (_, seen) = api(&router, "GET", &path, &bob, json!({})).await;
        assert_eq!(listed(&seen), vec![bob_id.clone()], "{seen}");
    }
    // Once Bob works with Alice, she is offered him for her other workspace;
    // Carol, whom she has never worked with, is not.
    let (status, _) = api(
        &router,
        "PUT",
        "workspace-access",
        &alice,
        json!({"workspace_id":team,"email":"bob@test.local","role":"editor"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, seen) = api(
        &router,
        "GET",
        &format!("workspaces/{second}/members"),
        &alice,
        json!({}),
    )
    .await;
    let ids = listed(&seen);
    assert!(ids.contains(&bob_id) && !ids.contains(&carol_id), "{seen}");
}

#[tokio::test]
async fn a_new_password_revokes_tokens_made_with_the_old_one() {
    let (router, root) = server().await;
    let session = common::member(&router, &root, "token@test.local").await;
    let make = |name: &'static str| {
        let router = router.clone();
        let session = session.clone();
        async move {
            let (status, made) = api(
                &router,
                "POST",
                "api-tokens",
                &session,
                json!({"name":name}),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{made}");
            made["token"].as_str().unwrap().to_string()
        }
    };
    let token = make("script").await;
    assert_eq!(
        api(&router, "GET", "bootstrap", &token, json!({})).await.0,
        StatusCode::OK
    );
    let (status, changed) = api(
        &router,
        "POST",
        "account/password",
        &session,
        json!({"current_password":PASSWORD,"new_password":"another-password-2"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(
        api(&router, "GET", "bootstrap", &token, json!({})).await.0,
        StatusCode::UNAUTHORIZED,
        "the token no longer works"
    );
    // The session that changed the password stays signed in.
    assert_eq!(
        api(&router, "GET", "bootstrap", &session, json!({}))
            .await
            .0,
        StatusCode::OK
    );

    // A password set by the administrator does the same.
    let token = make("again").await;
    let uid = user_id(&router, &session).await;
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("users/{uid}"),
        &root,
        json!({"password":"third-password-33"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        api(&router, "GET", "bootstrap", &token, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
}

fn new_document() -> String {
    let doc = Doc::new();
    {
        use yrs::Map;
        let meta = doc.get_or_insert_map("meta");
        let mut txn = doc.transact_mut();
        meta.insert(&mut txn, "icon", "▤");
        meta.insert(&mut txn, "favorite", false);
        meta.insert(&mut txn, "trashed", false);
    }
    let state = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    STANDARD.encode(state)
}
fn later() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    format!("{:013}.000000.replica", ms + 60_000)
}

#[tokio::test]
async fn a_synced_note_claims_only_records_of_its_own_folder() {
    let (router, root) = server().await;
    let victim = common::member(&router, &root, "victim@test.local").await;
    let attacker = common::member(&router, &root, "attacker@test.local").await;
    let folder_of = |boot: &Value| {
        common::team_folder(boot)["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (_, boot) = api(&router, "GET", "bootstrap", &victim, json!({})).await;
    let victim_folder = folder_of(&boot);
    let (_, db) = api(
        &router,
        "POST",
        "databases",
        &victim,
        json!({"folder_id":victim_folder,"name":"Salaries"}),
    )
    .await;
    let did = db["id"].as_str().unwrap().to_string();
    let name = db["columns"][0]["id"].as_str().unwrap().to_string();
    let (_, row) = api(
        &router,
        "POST",
        &format!("databases/{did}/rows"),
        &victim,
        json!({"cells":{&name:"Secret raise"}}),
    )
    .await;
    let rid = row["id"].as_str().unwrap().to_string();

    let (_, boot) = api(&router, "GET", "bootstrap", &attacker, json!({})).await;
    let ws = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = folder_of(&boot);
    let push = |ops: Value| {
        let router = router.clone();
        let (attacker, ws) = (attacker.clone(), ws.clone());
        async move {
            let (status, v) = api(
                &router,
                "POST",
                "sync/push",
                &attacker,
                json!({"workspace_id":ws,"ops":ops}),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{v}");
            v["results"][0].clone()
        }
    };
    let c = later();
    // A new note in the attacker's workspace, claiming the victim's record.
    let note = uuid::Uuid::new_v4().to_string();
    let result = push(json!([{"op_id":"a:1","kind":"note","id":note,"crdt":new_document(),
        "fields":{"folder_id":folder,"record":{"database_id":did,"record_id":rid}},"clocks":{"folder_id":c,"record":c}}])).await;
    assert_eq!(result["status"], "forbidden", "{result}");

    // Or an existing note of theirs, claiming it afterwards.
    let (_, plain) = api(
        &router,
        "POST",
        "notes",
        &attacker,
        json!({"folder_id":folder,"title":"Plain"}),
    )
    .await;
    let plain = plain["id"].as_str().unwrap().to_string();
    let result = push(json!([{"op_id":"a:2","kind":"note","id":plain,
        "fields":{"record":{"database_id":did,"record_id":rid}},"clocks":{"record":later()}}]))
    .await;
    assert_eq!(result["status"], "forbidden", "{result}");
    let (_, saved) = api(
        &router,
        "GET",
        &format!("notes/{plain}"),
        &attacker,
        json!({}),
    )
    .await;
    assert_eq!(saved["record"], Value::Null);
    assert_eq!(saved["title"], "Plain");

    // The victim's record still gets a page of its own.
    let (_, page) = api(
        &router,
        "POST",
        &format!("databases/{did}/rows/{rid}/page"),
        &victim,
        json!({}),
    )
    .await;
    let (status, opened) = api(
        &router,
        "GET",
        &format!("notes/{}", page["id"].as_str().unwrap()),
        &victim,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(opened["id"], note.as_str());

    // Pages of the attacker's own records sync as before, including for a
    // database that a later batch will bring.
    let (_, own) = api(
        &router,
        "POST",
        "databases",
        &attacker,
        json!({"folder_id":folder,"name":"Mine"}),
    )
    .await;
    let own = own["id"].as_str().unwrap().to_string();
    let c = later();
    for (op, database) in [
        ("a:3", own.clone()),
        ("a:4", uuid::Uuid::new_v4().to_string()),
    ] {
        let id = uuid::Uuid::new_v4().to_string();
        let result = push(json!([{"op_id":op,"kind":"note","id":id,"crdt":new_document(),
            "fields":{"folder_id":folder,"record":{"database_id":database,"record_id":uuid::Uuid::new_v4().to_string()}},"clocks":{"folder_id":c,"record":c}}])).await;
        assert_eq!(result["status"], "applied", "{result}");
    }
}
