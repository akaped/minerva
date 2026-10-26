//! Account management: administrators edit and delete accounts; everyone can
//! change their own password.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn call(r: &Router, method: &str, path: &str, token: &str, v: Value) -> (StatusCode, Value) {
    let response = r
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/api/v1/{path}"))
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
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
async fn login(r: &Router, email: &str, password: &str) -> (StatusCode, String) {
    let (status, v) = call(
        r,
        "POST",
        "login",
        "",
        json!({"email":email,"password":password}),
    )
    .await;
    (status, v["token"].as_str().unwrap_or("").into())
}

#[tokio::test]
async fn administrators_edit_and_delete_accounts_and_people_change_passwords() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    let (_, user) = call(&r, "POST", "users", &admin, json!({"name":"Ada","email":"ada@test.local","password":"ada-password-1234","can_create_workspace":true})).await;
    let uid = user["id"].as_str().unwrap().to_string();
    call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Bob","email":"bob@test.local","password":"bob-password-1234"}),
    )
    .await;

    // Edit: name, email, administrator; duplicates and self-edits are refused.
    let (status, edited) = call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"name":"Ada Lovelace","email":"Ada.L@Test.local"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(
        (edited["name"].as_str(), edited["email"].as_str()),
        (Some("Ada Lovelace"), Some("ada.l@test.local"))
    );
    let (status, _) = call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"email":"bob@test.local"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, me) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    let admin_id = me["user"]["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        &r,
        "PATCH",
        &format!("users/{admin_id}"),
        &admin,
        json!({"name":"Me"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, promoted) = call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"admin":true}),
    )
    .await;
    assert_eq!(
        (status, promoted["admin"].as_bool()),
        (StatusCode::OK, Some(true))
    );
    call(
        &r,
        "PATCH",
        &format!("users/{uid}"),
        &admin,
        json!({"admin":false}),
    )
    .await;

    // Changing your own password needs the current one, ends other sessions,
    // and keeps this one.
    let (_, ada) = login(&r, "ada.l@test.local", "ada-password-1234").await;
    let (_, ada_phone) = login(&r, "ada.l@test.local", "ada-password-1234").await;
    let (status, _) = call(
        &r,
        "POST",
        "account/password",
        &ada,
        json!({"current_password":"wrong-password-1234","new_password":"new-ada-password-1"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &r,
        "POST",
        "account/password",
        &ada,
        json!({"current_password":"ada-password-1234","new_password":"short"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &r,
        "POST",
        "account/password",
        &ada,
        json!({"current_password":"ada-password-1234","new_password":"new-ada-password-1"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&r, "GET", "bootstrap", &ada, json!({})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&r, "GET", "bootstrap", &ada_phone, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&r, "ada.l@test.local", "ada-password-1234").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&r, "ada.l@test.local", "new-ada-password-1").await.0,
        StatusCode::OK
    );

    // Ada owns a workspace and has a private deck and chat.
    let (_, workspace) = call(
        &r,
        "POST",
        "workspaces",
        &ada,
        json!({"name":"Ada's research"}),
    )
    .await;
    let (status, _) = call(
        &r,
        "POST",
        "flashcards/decks",
        &ada,
        json!({"name":"Private deck"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = call(
        &r,
        "POST",
        "ai/chats",
        &ada,
        json!({"scope":{"kind":"workspace","id":workspace["id"]}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Delete: not yourself, not by members; the account's sign-ins, private
    // data, and unshared workspaces go.
    let (status, _) = call(
        &r,
        "DELETE",
        &format!("users/{admin_id}"),
        &admin,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&r, "DELETE", &format!("users/{admin_id}"), &ada, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, deleted) = call(&r, "DELETE", &format!("users/{uid}"), &admin, json!({})).await;
    assert_eq!(
        (
            status,
            deleted["workspaces_transferred"].as_u64(),
            deleted["workspaces_deleted"].as_u64()
        ),
        (StatusCode::OK, Some(0), Some(2)),
        "{deleted}"
    );
    assert_eq!(
        call(&r, "GET", "bootstrap", &ada, json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&r, "ada.l@test.local", "new-ada-password-1").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    // Her personal workspace was not shared: it is gone, not handed to the
    // administrator.
    assert!(!boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == workspace["id"]));
    let (_, list) = call(&r, "GET", "admin", &admin, json!({})).await;
    assert!(!list["users"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["id"] == uid.as_str()));
    // The same address can be used again, with nothing of the old account.
    let (_, again) = call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Ada","email":"ada.l@test.local","password":"ada-password-1234"}),
    )
    .await;
    assert!(again["id"].is_string());
    let (_, ada) = login(&r, "ada.l@test.local", "ada-password-1234").await;
    let (_, decks) = call(&r, "GET", "flashcards/decks", &ada, json!({})).await;
    assert_eq!(decks["decks"], json!([]));
    let (_, chats) = call(&r, "GET", "ai/chats", &ada, json!({})).await;
    assert_eq!(chats, json!([]));
}

#[tokio::test]
async fn new_members_choose_a_password_and_have_a_private_my_workspace() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    assert_eq!(
        boot["must_change_password"], false,
        "the first administrator chose their password"
    );
    let names: Vec<_> = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| (w["name"].as_str().unwrap(), w["personal"] == true))
        .collect();
    // Administrators manage the server: they have no workspaces of their own.
    assert!(names.is_empty(), "{names:?}");

    // Each account has its own My Workspace from the start, opened by default.
    let (_, user) = call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Cleo","email":"cleo@test.local","password":"temporary-pass-1234"}),
    )
    .await;
    let (_, cleo) = login(&r, "cleo@test.local", "temporary-pass-1234").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &cleo, json!({})).await;
    assert_eq!(boot["must_change_password"], true);
    let mine = &boot["workspaces"][0];
    assert_eq!(
        (
            mine["name"].as_str(),
            mine["personal"].as_bool(),
            mine["owner_id"].as_str()
        ),
        (Some("My Workspace"), Some(true), user["id"].as_str())
    );
    let personal = mine["id"].as_str().unwrap().to_string();
    assert_eq!(boot["default_workspace"], personal.as_str());
    assert!(
        boot["folders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["workspace_id"] == personal.as_str()),
        "with a first folder"
    );
    let (status, _) = call(
        &r,
        "POST",
        "account/password",
        &cleo,
        json!({"current_password":"temporary-pass-1234","new_password":"cleos-own-password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, boot) = call(&r, "GET", "bootstrap", &cleo, json!({})).await;
    assert_eq!(boot["must_change_password"], false);

    // It cannot be shared, renamed, or deleted, by its owner or anyone else.
    let (_, other) = call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Dan","email":"dan@test.local","password":"dan-password-1234"}),
    )
    .await;
    let (status, error) = call(
        &r,
        "PUT",
        "workspace-access",
        &cleo,
        json!({"workspace_id":personal,"user_id":other["id"],"role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("personal"));
    let folder = boot["folders"][0]["id"].clone();
    let (status, _) = call(
        &r,
        "PUT",
        "permissions",
        &cleo,
        json!({"folder_id":folder,"user_id":other["id"],"role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &r,
        "PATCH",
        &format!("workspaces/{personal}"),
        &cleo,
        json!({"name":"Renamed"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(boot["workspaces"][0]["can_delete"], false);
    let (status, _) = call(
        &r,
        "DELETE",
        &format!("workspaces/{personal}"),
        &cleo,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Other workspaces are renamed by their owner only.
    let (_, dan) = login(&r, "dan@test.local", "dan-password-1234").await;
    let (_, created) = call(&r, "POST", "workspaces", &dan, json!({"name":"Team"})).await;
    let team = created["id"].as_str().unwrap().to_string();
    let (status, renamed) = call(
        &r,
        "PATCH",
        &format!("workspaces/{team}"),
        &dan,
        json!({"name":"Research"}),
    )
    .await;
    assert_eq!(
        (status, renamed["name"].as_str()),
        (StatusCode::OK, Some("Research"))
    );
    let (status, _) = call(
        &r,
        "PATCH",
        &format!("workspaces/{team}"),
        &cleo,
        json!({"name":"Mine"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &r,
        "PATCH",
        &format!("workspaces/{personal}"),
        &admin,
        json!({"name":"Peek"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "administrators do not rename other people's workspaces"
    );

    // A password set by an administrator must be replaced again.
    call(
        &r,
        "PATCH",
        &format!("users/{}", user["id"].as_str().unwrap()),
        &admin,
        json!({"password":"reset-by-admin-123"}),
    )
    .await;
    let (_, cleo) = login(&r, "cleo@test.local", "reset-by-admin-123").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &cleo, json!({})).await;
    assert_eq!(boot["must_change_password"], true);
}
#[tokio::test]
async fn shared_workspaces_of_a_deleted_account_pass_to_a_successor() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    let (_, eve) = call(&r, "POST", "users", &admin, json!({"name":"Eve","email":"eve@test.local","password":"eve-password-1234","can_create_workspace":true})).await;
    let (_, finn) = call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Finn","email":"finn@test.local","password":"finn-password-123"}),
    )
    .await;
    let (_, eve_token) = login(&r, "eve@test.local", "eve-password-1234").await;
    let (_, team) = call(&r, "POST", "workspaces", &eve_token, json!({"name":"Team"})).await;
    let (_, private) = call(
        &r,
        "POST",
        "workspaces",
        &eve_token,
        json!({"name":"Diary"}),
    )
    .await;
    // The administrator cannot see either of them.
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    assert!(!boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == team["id"] || w["id"] == private["id"]));
    let (status, _) = call(
        &r,
        "PUT",
        "workspace-access",
        &eve_token,
        json!({"workspace_id":team["id"],"user_id":finn["id"],"role":"editor"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // A member must take over the shared workspace first.
    let (status, _) = call(
        &r,
        "DELETE",
        &format!("users/{}", eve["id"].as_str().unwrap()),
        &admin,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, deleted) = call(
        &r,
        "DELETE",
        &format!("users/{}", eve["id"].as_str().unwrap()),
        &admin,
        json!({"successor_id": finn["id"]}),
    )
    .await;
    assert_eq!(
        (
            deleted["workspaces_transferred"].as_u64(),
            deleted["workspaces_deleted"].as_u64()
        ),
        (Some(1), Some(2))
    );
    // The administrator still sees no workspaces; the successor owns Team.
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    assert!(boot["workspaces"].as_array().unwrap().is_empty());
    let (_, finn_token) = login(&r, "finn@test.local", "finn-password-123").await;
    let (_, boot) = call(&r, "GET", "bootstrap", &finn_token, json!({})).await;
    let kept = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == team["id"])
        .expect("the successor keeps it");
    assert_eq!(kept["owner_id"], finn["id"]);
    assert!(!boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == private["id"]));
}

#[tokio::test]
async fn each_account_keeps_its_own_dashboard_layout() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    let (_, empty) = call(&r, "GET", "account/dashboard", &admin, json!({})).await;
    assert_eq!(
        empty["widgets"],
        Value::Null,
        "the default layout until changed"
    );
    let layout = json!([
        {"id":"a1","type":"recent","size":"l","options":{}},
        {"id":"b2","type":"chart","size":"m","options":{"database":"x","column":"y","kind":"pie"}}
    ]);
    let (status, saved) = call(
        &r,
        "PUT",
        "account/dashboard",
        &admin,
        json!({"widgets":layout}),
    )
    .await;
    assert_eq!((status, &saved["widgets"]), (StatusCode::OK, &layout));
    assert_eq!(
        call(&r, "GET", "account/dashboard", &admin, json!({}))
            .await
            .1["widgets"],
        layout
    );
    for bad in [
        json!([{"id":"x","type":"rocket","size":"s"}]),
        json!([{"id":"x","type":"recent","size":"huge"}]),
        json!([{"id":"","type":"recent","size":"s"}]),
        json!((0..41)
            .map(|i| json!({"id":i.to_string(),"type":"recent","size":"s"}))
            .collect::<Vec<_>>()),
    ] {
        assert_eq!(
            call(
                &r,
                "PUT",
                "account/dashboard",
                &admin,
                json!({"widgets":bad})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    // Another account has its own; null restores the default.
    call(
        &r,
        "POST",
        "users",
        &admin,
        json!({"name":"Gil","email":"gil@test.local","password":"gil-password-1234"}),
    )
    .await;
    let (_, gil) = login(&r, "gil@test.local", "gil-password-1234").await;
    assert_eq!(
        call(&r, "GET", "account/dashboard", &gil, json!({}))
            .await
            .1["widgets"],
        Value::Null
    );
    call(
        &r,
        "PUT",
        "account/dashboard",
        &admin,
        json!({"widgets":null}),
    )
    .await;
    assert_eq!(
        call(&r, "GET", "account/dashboard", &admin, json!({}))
            .await
            .1["widgets"],
        Value::Null
    );
}

/// Administrators run the server: accounts, settings and statistics, never
/// notes. Members create workspaces and share them with each other; editors
/// of a shared workspace manage it like its owner.
#[tokio::test]
async fn administrators_manage_and_members_share_workspaces() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    for (name, email) in [("Gil", "gil@test.local"), ("Hana", "hana@test.local")] {
        call(
            &r,
            "POST",
            "users",
            &admin,
            json!({"name":name,"email":email,"password":"member-password-1"}),
        )
        .await;
    }

    // No workspaces, and no way to make one or reach content.
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    assert!(boot["workspaces"].as_array().unwrap().is_empty());
    let (status, error) = call(&r, "POST", "workspaces", &admin, json!({"name":"Mine"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        error["error"].as_str().unwrap().contains("member account"),
        "{error}"
    );
    // Statistics instead.
    let (status, stats) = call(&r, "GET", "admin/overview", &admin, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{stats}");
    assert_eq!(stats["total_users"], 3);
    assert_eq!(
        stats["total_workspaces"], 2,
        "the members' own My Workspaces"
    );

    // A member creates a workspace and picks a colleague from the members list.
    let (_, gil) = login(&r, "gil@test.local", "member-password-1").await;
    let (_, hana) = login(&r, "hana@test.local", "member-password-1").await;
    let (status, project) = call(&r, "POST", "workspaces", &gil, json!({"name":"Project"})).await;
    assert_eq!(status, StatusCode::OK);
    let wid = project["id"].as_str().unwrap();
    let (_, members) = call(
        &r,
        "GET",
        &format!("workspaces/{wid}/members"),
        &gil,
        json!({}),
    )
    .await;
    let people: Vec<_> = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["email"].as_str().unwrap())
        .collect();
    assert!(
        people.contains(&"hana@test.local") && !people.contains(&"admin@test.local"),
        "{people:?}"
    );
    let hana_id = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["email"] == "hana@test.local")
        .unwrap()["id"]
        .clone();
    let (status, _) = call(
        &r,
        "PUT",
        "workspace-access",
        &gil,
        json!({"workspace_id":wid,"user_id":hana_id,"role":"editor"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The editor manages folders and the workspace's public sites.
    let (status, folder) = call(
        &r,
        "POST",
        "folders",
        &hana,
        json!({"workspace_id":wid,"name":"Research"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{folder}");
    let (status, site) = call(
        &r,
        "POST",
        "sites",
        &hana,
        json!({"workspace_id":wid,"path":"project","title":"Project"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{site}");
    let (status, _) = call(
        &r,
        "DELETE",
        &format!("sites/{}", site["id"].as_str().unwrap()),
        &hana,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &r,
        "DELETE",
        &format!("folders/{}", folder["id"].as_str().unwrap()),
        &hana,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // My Workspace stays private.
    let (_, boot) = call(&r, "GET", "bootstrap", &gil, json!({})).await;
    let personal = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["personal"] == true)
        .unwrap()["id"]
        .clone();
    let (status, _) = call(
        &r,
        "PUT",
        "workspace-access",
        &gil,
        json!({"workspace_id":personal,"user_id":hana_id,"role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The `@` picker offers the people who can open a note, and a mention shows
/// up in the mentioned person's list.
#[tokio::test]
async fn mentions_offer_people_who_can_open_the_note() {
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let (_, admin) = login(&r, "admin@test.local", "test-password-1234").await;
    for (name, email) in [
        ("Gil", "gil@test.local"),
        ("Hana", "hana@test.local"),
        ("Ivo", "ivo@test.local"),
    ] {
        call(
            &r,
            "POST",
            "users",
            &admin,
            json!({"name":name,"email":email,"password":"member-password-1"}),
        )
        .await;
    }
    let (_, gil) = login(&r, "gil@test.local", "member-password-1").await;
    let (_, hana) = login(&r, "hana@test.local", "member-password-1").await;
    let (_, ivo) = login(&r, "ivo@test.local", "member-password-1").await;
    let (_, project) = call(&r, "POST", "workspaces", &gil, json!({"name":"Project"})).await;
    let wid = project["id"].as_str().unwrap();
    let (_, members) = call(
        &r,
        "GET",
        &format!("workspaces/{wid}/members"),
        &gil,
        json!({}),
    )
    .await;
    let hana_id = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "Hana")
        .unwrap()["id"]
        .clone();
    call(
        &r,
        "PUT",
        "workspace-access",
        &gil,
        json!({"workspace_id":wid,"user_id":hana_id,"role":"editor"}),
    )
    .await;
    let (_, boot) = call(&r, "GET", "bootstrap", &gil, json!({})).await;
    let folder = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] == wid)
        .unwrap()["id"]
        .clone();
    let (_, note) = call(
        &r,
        "POST",
        "notes",
        &gil,
        json!({"folder_id":folder,"title":"Plan"}),
    )
    .await;
    let nid = note["id"].as_str().unwrap();

    // Gil and Hana can open it; Ivo and the administrator cannot.
    let (status, people) = call(&r, "GET", &format!("notes/{nid}/people"), &gil, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<_> = people
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Gil", "Hana"]);
    let (status, _) = call(&r, "GET", &format!("notes/{nid}/people"), &ivo, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, mut full) = call(&r, "GET", &format!("notes/{nid}"), &gil, json!({})).await;
    full["blocks"] = json!([{"id":"b1","markdown":"Review this @hana@test.local","x":0,"y":0}]);
    call(&r, "PUT", &format!("notes/{nid}"), &gil, full).await;
    let (_, mentions) = call(&r, "GET", "mentions", &hana, json!({})).await;
    assert_eq!(mentions[0]["title"], "Plan");
    assert!(mentions[0]["excerpt"]
        .as_str()
        .unwrap()
        .contains("Review this"));
    let (_, none) = call(&r, "GET", "mentions", &gil, json!({})).await;
    assert!(none.as_array().unwrap().is_empty());
}
