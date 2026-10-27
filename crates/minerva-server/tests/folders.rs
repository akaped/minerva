//! Subfolders, moving notes and folders, and inherited folder access.
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

async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(format!("/api/v1/{path}"))
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
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
async fn user(router: &Router, admin: &str, name: &str) -> (String, String) {
    let email = format!("{name}@test.local");
    let (_, v) = api(
        router,
        "POST",
        "users",
        admin,
        json!({"name":name,"email":email,"password":PASSWORD,"can_create_workspace":true}),
    )
    .await;
    (
        v["id"].as_str().unwrap().to_string(),
        login(router, &email).await,
    )
}
async fn folder(
    router: &Router,
    token: &str,
    wid: &str,
    name: &str,
    parent: Option<&str>,
) -> (StatusCode, String) {
    let (status, f) = api(
        router,
        "POST",
        "folders",
        token,
        json!({"workspace_id":wid,"name":name,"parent_id":parent}),
    )
    .await;
    (status, f["id"].as_str().unwrap_or_default().to_string())
}
async fn note(router: &Router, token: &str, folder: &str, title: &str) -> String {
    let (_, n) = api(
        router,
        "POST",
        "notes",
        token,
        json!({"folder_id":folder,"title":title}),
    )
    .await;
    n["id"].as_str().unwrap().to_string()
}
fn ids(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["id"].as_str())
        .collect()
}

#[tokio::test]
async fn folders_nest_notes_move_and_grants_cover_subfolders() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let (_, alice) = user(&router, &root, "alice").await;
    let (carol_id, carol) = user(&router, &root, "carol").await;
    let (_, w) = api(
        &router,
        "POST",
        "workspaces",
        &alice,
        json!({"name":"Alpha"}),
    )
    .await;
    let wid = w["id"].as_str().unwrap();
    let (_, other) = api(
        &router,
        "POST",
        "workspaces",
        &alice,
        json!({"name":"Other"}),
    )
    .await;
    let (_, other_folder) = folder(
        &router,
        &alice,
        other["id"].as_str().unwrap(),
        "Elsewhere",
        None,
    )
    .await;

    // Nesting, within one workspace, without cycles, and bounded in depth.
    let (status, projects) = folder(&router, &alice, wid, "Projects", None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, aurora) = folder(&router, &alice, wid, "Aurora", Some(&projects)).await;
    let (_, research) = folder(&router, &alice, wid, "Research", Some(&aurora)).await;
    let (_, boot) = api(&router, "GET", "bootstrap", &alice, json!({})).await;
    let found = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == research.as_str())
        .unwrap();
    assert_eq!(found["parent_id"], aurora.as_str());
    assert_eq!(
        folder(&router, &alice, wid, "x", Some(&other_folder))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("folders/{projects}"),
        &alice,
        json!({"parent_id":research}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "no cycles");
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("folders/{projects}"),
        &alice,
        json!({"parent_id":projects}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let mut parent = research.clone();
    for depth in 3..10 {
        let (status, id) = folder(&router, &alice, wid, "deep", Some(&parent)).await;
        if depth < 8 {
            assert_eq!(status, StatusCode::OK, "depth {depth}");
            parent = id;
        } else {
            assert_eq!(status, StatusCode::BAD_REQUEST, "depth {depth}");
            break;
        }
    }
    let (status, moved) = api(
        &router,
        "PATCH",
        &format!("folders/{research}"),
        &alice,
        json!({"parent_id":null}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(moved.get("parent_id").is_none(), "back at the top level");
    let (_, renamed) = api(
        &router,
        "PATCH",
        &format!("folders/{research}"),
        &alice,
        json!({"name":"Lab"}),
    )
    .await;
    assert_eq!(renamed["name"], "Lab");
    api(
        &router,
        "PATCH",
        &format!("folders/{research}"),
        &alice,
        json!({"parent_id":aurora}),
    )
    .await;

    // An older folder grant covers that folder's subfolders.
    let deep_note = note(&router, &alice, &research, "Findings").await;
    api(
        &router,
        "PUT",
        "permissions",
        &alice,
        json!({"folder_id":projects,"user_id":carol_id,"role":"viewer"}),
    )
    .await;
    let (_, boot) = api(&router, "GET", "bootstrap", &carol, json!({})).await;
    let visible = ids(&boot["folders"]);
    assert!(
        visible.contains(&projects.as_str())
            && visible.contains(&aurora.as_str())
            && visible.contains(&research.as_str())
    );
    assert_eq!(boot["roles"][&research], "viewer");
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{deep_note}"),
            &carol,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        api(
            &router,
            "PATCH",
            &format!("folders/{aurora}"),
            &carol,
            json!({"name":"x"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // Moving notes: folder editors only, within the workspace.
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("notes/{deep_note}"),
        &carol,
        json!({"folder_id":projects}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "viewers cannot move notes");
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("notes/{deep_note}"),
        &alice,
        json!({"folder_id":other_folder}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "notes stay in their workspace"
    );
    let (status, view) = api(
        &router,
        "PATCH",
        &format!("notes/{deep_note}"),
        &alice,
        json!({"folder_id":projects}),
    )
    .await;
    assert_eq!(
        (status, view["folder_id"].as_str()),
        (StatusCode::OK, Some(projects.as_str()))
    );
    api(
        &router,
        "POST",
        &format!("notes/{deep_note}/shares"),
        &alice,
        json!({"kind":"member","email":"carol@test.local","role":"editor"}),
    )
    .await;
    api(
        &router,
        "PUT",
        "permissions",
        &alice,
        json!({"folder_id":projects,"user_id":carol_id,"role":"none"}),
    )
    .await;
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("notes/{deep_note}"),
        &carol,
        json!({"folder_id":aurora}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "share recipients cannot move notes away"
    );

    // Deleting a folder deletes its subfolders and their notes.
    let inner = note(&router, &alice, &research, "Inner").await;
    let (status, done) = api(
        &router,
        "DELETE",
        &format!("folders/{aurora}"),
        &alice,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, done["deleted_notes"].as_u64()),
        (StatusCode::OK, Some(1))
    );
    let (_, boot) = api(&router, "GET", "bootstrap", &alice, json!({})).await;
    let left = ids(&boot["folders"]);
    assert!(
        left.contains(&projects.as_str())
            && !left.contains(&aurora.as_str())
            && !left.contains(&research.as_str())
    );
    assert!(!left.contains(&parent.as_str()), "deep descendants go too");
    assert_eq!(
        api(&router, "GET", &format!("notes/{inner}"), &alice, json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(
            &router,
            "GET",
            &format!("notes/{deep_note}"),
            &alice,
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
}

/// Members who can edit a shared workspace add folders and start its journal;
/// members who can only view it cannot.
#[tokio::test]
async fn workspace_editors_add_folders_and_journals() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    for (name, email) in [("Ed", "ed@test.local"), ("Vi", "vi@test.local")] {
        api(
            &router,
            "POST",
            "users",
            &root,
            json!({"name":name,"email":email,"password":PASSWORD}),
        )
        .await;
    }
    let (_, created) = api(
        &router,
        "POST",
        "workspaces",
        &admin,
        json!({"name":"Team"}),
    )
    .await;
    let team = created["id"].as_str().unwrap().to_string();
    let (_, admin_view) = api(&router, "GET", "admin", &root, json!({})).await;
    let id_of = |email: &str| {
        admin_view["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["email"] == email)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    for (email, role) in [("ed@test.local", "editor"), ("vi@test.local", "viewer")] {
        let (status, _) = api(
            &router,
            "PUT",
            "workspace-access",
            &admin,
            json!({"workspace_id":team,"user_id":id_of(email),"role":role}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let editor = login(&router, "ed@test.local").await;
    let viewer = login(&router, "vi@test.local").await;
    // Accounts created by an administrator must change their password first.
    for token in [&editor, &viewer] {
        api(
            &router,
            "POST",
            "account/password",
            token,
            json!({"current_password":PASSWORD,"new_password":PASSWORD}),
        )
        .await;
    }

    let (_, boot) = api(&router, "GET", "bootstrap", &editor, json!({})).await;
    let shared = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == team.as_str())
        .unwrap();
    assert_eq!(shared["can_edit"], true);
    let (status, folder) = api(
        &router,
        "POST",
        "folders",
        &editor,
        json!({"workspace_id":team,"name":"Ed's folder"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{folder}");
    let (status, sub) = api(
        &router,
        "POST",
        "folders",
        &editor,
        json!({"workspace_id":team,"name":"Inside","parent_id":folder["id"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sub}");
    let (status, day) = api(
        &router,
        "POST",
        "journal",
        &editor,
        json!({"workspace_id":team,"date":"2026-10-26"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{day}");
    // Editors also rename folders.
    let fid = folder["id"].as_str().unwrap();
    let (status, _) = api(
        &router,
        "PATCH",
        &format!("folders/{fid}"),
        &editor,
        json!({"name":"Renamed"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, boot) = api(&router, "GET", "bootstrap", &viewer, json!({})).await;
    let shared = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == team.as_str())
        .unwrap();
    assert_eq!(shared["can_edit"], false);
    let (status, _) = api(
        &router,
        "POST",
        "folders",
        &viewer,
        json!({"workspace_id":team,"name":"Nope"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
