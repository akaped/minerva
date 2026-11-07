//! Personal API tokens: they act as their owner, expire and can be revoked, and
//! cannot manage the account.
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
async fn tokens_act_as_their_owner_and_can_be_revoked() {
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
    let other = common::account(&router, &root, "other@test.local").await;

    // Administrators cannot have tokens; names and expiry are checked.
    let (status, _) = api(
        &router,
        "POST",
        "api-tokens",
        &root,
        json!({"name":"Admin"}),
    )
    .await;
    assert!(status == 400 || status == 403, "{status}");
    for bad in [
        json!({"name":""}),
        json!({"name":"x","expires_days":0}),
        json!({"name":"x","expires_days":400}),
    ] {
        let (status, _) = api(&router, "POST", "api-tokens", &owner, bad.clone()).await;
        assert_eq!(status, 400, "{bad}");
    }
    let (status, created) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name":"Laptop script","expires_days":30}),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let token = created["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("mnv_api_v1_"));
    let (_, listed) = api(&router, "GET", "api-tokens", &owner, json!({})).await;
    assert_eq!(listed[0]["name"], "Laptop script");
    assert!(listed[0].get("token").is_none() && listed[0].get("token_hash").is_none());
    let (_, others) = api(&router, "GET", "api-tokens", &other, json!({})).await;
    assert!(others.as_array().unwrap().is_empty());

    // The token reads and writes as its owner.
    let (status, boot) = api(&router, "GET", "bootstrap", &token, json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(boot["user"]["email"], "owner@test.local");
    let folder = common::team_folder(&boot)["id"].clone();
    let (status, note) = api(
        &router,
        "POST",
        "notes",
        &token,
        json!({"folder_id":folder,"title":"From a script"}),
    )
    .await;
    assert_eq!(status, 200, "{note}");
    let (status, found) = api(&router, "POST", "search", &token, json!({"query":"script"})).await;
    assert_eq!(status, 200);
    assert!(found.to_string().contains("From a script"));

    // It cannot manage the account.
    for (method, path) in [
        ("GET", "api-tokens"),
        ("POST", "api-tokens"),
        ("GET", "mcp-credentials"),
        ("POST", "account/password"),
        ("GET", "account/2fa"),
        ("POST", "logout"),
    ] {
        let (status, _) = api(&router, method, path, &token, json!({"name":"x"})).await;
        assert_eq!(status, 403, "{method} {path}");
    }

    // Someone else cannot revoke it; its owner can, and then it stops working.
    let path = format!("api-tokens/{}", created["id"].as_str().unwrap());
    let (status, _) = api(&router, "DELETE", &path, &other, json!({})).await;
    assert_eq!(status, 404);
    let (status, _) = api(&router, "DELETE", &path, &owner, json!({})).await;
    assert_eq!(status, 200);
    let (status, _) = api(&router, "GET", "bootstrap", &token, json!({})).await;
    assert_eq!(status, 401);
    let (status, _) = api(&router, "GET", "bootstrap", "mnv_api_v1_made-up", json!({})).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn workspace_tokens_cannot_escape_through_reads_writes_or_account_routes() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap();
    let owner = common::member(&router, root, "scope@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let workspace = common::team_workspace(&boot)["id"].clone();
    let allowed_folder = common::team_folder(&boot)["id"].clone();
    let other_folder = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] != workspace)
        .unwrap()["id"]
        .clone();
    let (_, outside) = api(&router, "POST", "notes", &owner, json!({"folder_id":other_folder,"title":"Secret outside scope","markdown":"private contents"})).await;
    let outside_id = outside["id"].as_str().unwrap();
    let (_, created) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name":"Scoped","workspace_id":workspace}),
    )
    .await;
    assert_eq!(created["workspace_id"], workspace);
    let token = created["token"].as_str().unwrap();
    let (status, scoped) = api(&router, "GET", "bootstrap", token, json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(scoped["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(scoped["workspaces"][0]["id"], workspace);
    assert!(!scoped.to_string().contains("Secret outside scope"));
    assert!(scoped["user"].get("token_workspace").is_none());
    let (_, search) = api(&router, "POST", "search", token, json!({"query":"Secret"})).await;
    assert!(search.as_array().unwrap().is_empty());
    for (method, path, body) in [
        ("GET", format!("notes/{outside_id}"), json!({})),
        ("GET", format!("notes/{outside_id}/history"), json!({})),
        ("GET", format!("notes/{outside_id}/comments"), json!({})),
        (
            "POST",
            format!("notes/{outside_id}/append"),
            json!({"markdown":"tampered"}),
        ),
        (
            "POST",
            "notes".into(),
            json!({"folder_id":other_folder,"title":"escape"}),
        ),
        (
            "PATCH",
            format!("folders/{}", other_folder.as_str().unwrap()),
            json!({"name":"escape"}),
        ),
        ("POST", "workspaces".into(), json!({"name":"escape"})),
        ("GET", "reminders".into(), json!({})),
        ("GET", "flashcards/decks".into(), json!({})),
        ("GET", "sharing".into(), json!({})),
        ("GET", "sync/changes?workspace=x".into(), json!({})),
        ("POST", "sync/push".into(), json!({})),
        ("GET", "live/wait".into(), json!({})),
        ("POST", "api-tokens".into(), json!({"name":"escape"})),
    ] {
        let (status, result) = api(&router, method, &path, token, body).await;
        assert_eq!(status, 403, "{method} {path}: {result}");
    }
    let (status, allowed) = api(
        &router,
        "POST",
        "notes",
        token,
        json!({"folder_id":allowed_folder,"title":"In scope"}),
    )
    .await;
    assert_eq!(status, 200, "{allowed}");
    let (status, _) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name":"Invalid","workspace_id":"missing"}),
    )
    .await;
    assert_eq!(status, 403);
    let (_, readonly) = api(
        &router,
        "POST",
        "api-tokens",
        &owner,
        json!({"name":"Read scoped","workspace_id":workspace,"access":"read_only"}),
    )
    .await;
    let readonly = readonly["token"].as_str().unwrap();
    assert_eq!(
        api(
            &router,
            "POST",
            "search",
            readonly,
            json!({"query":"scope"})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        api(
            &router,
            "POST",
            "notes",
            readonly,
            json!({"folder_id":allowed_folder,"title":"denied"})
        )
        .await
        .0,
        403
    );
}

#[tokio::test]
async fn scoped_tokens_respect_revocation_and_do_not_inherit_outside_note_shares() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap();
    let owner = common::member(&router, root, "owner@test.local").await;
    let member = common::account(&router, root, "member@test.local").await;
    let (_, mb) = api(&router, "GET", "bootstrap", &member, json!({})).await;
    let uid = mb["user"]["id"].clone();
    let (_, boot) = api(&router, "GET", "bootstrap", &owner, json!({})).await;
    let workspace = common::team_workspace(&boot)["id"].clone();
    let note = common::welcome(&boot)["id"].as_str().unwrap();
    assert_eq!(
        api(
            &router,
            "PUT",
            "workspace-access",
            &owner,
            json!({"workspace_id":workspace,"user_id":uid,"role":"viewer"})
        )
        .await
        .0,
        200
    );
    let outside_folder = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] != workspace)
        .unwrap()["id"]
        .clone();
    let (_, outside) = api(
        &router,
        "POST",
        "notes",
        &owner,
        json!({"folder_id":outside_folder,"title":"Directly shared outside"}),
    )
    .await;
    let outside_id = outside["id"].as_str().unwrap();
    assert_eq!(
        api(
            &router,
            "POST",
            &format!("notes/{outside_id}/shares"),
            &owner,
            json!({"kind":"member","email":"member@test.local","role":"viewer"})
        )
        .await
        .0,
        200
    );
    let (_, created) = api(
        &router,
        "POST",
        "api-tokens",
        &member,
        json!({"name":"Workspace only","workspace_id":workspace}),
    )
    .await;
    let token = created["token"].as_str().unwrap();
    assert_eq!(
        api(&router, "GET", &format!("notes/{note}"), token, json!({}))
            .await
            .0,
        200
    );
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{outside_id}"),
            &member,
            json!({})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{outside_id}"),
            token,
            json!({})
        )
        .await
        .0,
        403
    );
    let (_, scoped) = api(&router, "GET", "bootstrap", token, json!({})).await;
    assert!(scoped["shared_notes"].as_array().unwrap().is_empty());
    assert_eq!(
        api(
            &router,
            "PUT",
            "workspace-access",
            &owner,
            json!({"workspace_id":workspace,"user_id":uid,"role":"none"})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        api(&router, "GET", &format!("notes/{note}"), token, json!({}))
            .await
            .0,
        403
    );
    let (_, scoped) = api(&router, "GET", "bootstrap", token, json!({})).await;
    assert!(scoped["workspaces"].as_array().unwrap().is_empty());
    // The unchanged legacy token remains unscoped.
    let (_, legacy) = api(
        &router,
        "POST",
        "api-tokens",
        &member,
        json!({"name":"All"}),
    )
    .await;
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{outside_id}"),
            legacy["token"].as_str().unwrap(),
            json!({})
        )
        .await
        .0,
        200
    );
}
