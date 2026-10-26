//! Tenant separation: users in different workspaces see nothing of each other
//! unless an administrator grants folder access or a folder editor shares a note.
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
    body: Body,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(body)
        .unwrap();
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
    send(
        router,
        method,
        &format!("/api/v1/{path}"),
        token,
        Body::from(body.to_string()),
    )
    .await
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
async fn user(router: &Router, admin: &str, name: &str, creator: bool) -> (String, String) {
    let email = format!("{}@test.local", name.to_lowercase());
    let (status, v) = api(
        router,
        "POST",
        "users",
        admin,
        json!({"name":name,"email":email,"password":PASSWORD,"can_create_workspace":creator}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    (
        v["id"].as_str().unwrap().to_string(),
        login(router, &email).await,
    )
}
/// A workspace with one note (with an attachment), a database, and a template.
struct Tenant {
    workspace: String,
    folder: String,
    note: String,
    attachment: String,
    database: String,
    template: String,
}
async fn tenant(router: &Router, token: &str, name: &str, secret: &str) -> Tenant {
    let (_, w) = api(router, "POST", "workspaces", token, json!({"name":name})).await;
    let workspace = w["id"].as_str().unwrap().to_string();
    let (_, boot) = api(router, "GET", "bootstrap", token, json!({})).await;
    let folder = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] == workspace)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, mut note) = api(
        router,
        "POST",
        "notes",
        token,
        json!({"folder_id":folder,"title":format!("{secret} plan")}),
    )
    .await;
    let note_id = note["id"].as_str().unwrap().to_string();
    note["blocks"] =
        json!([{"id":"b1","markdown":format!("The {secret} launch code\n- [ ] {secret} task")}]);
    let (status, _) = api(router, "PUT", &format!("notes/{note_id}"), token, note).await;
    assert_eq!(status, StatusCode::OK);
    let (status, a) = send(
        router,
        "POST",
        &format!("/api/v1/notes/{note_id}/attachments?filename=secret.txt"),
        token,
        Body::from(secret.to_string()),
    )
    .await;
    assert!(status.is_success(), "{a}");
    let (_, db) = api(
        router,
        "POST",
        "databases",
        token,
        json!({"folder_id":folder,"name":format!("{secret} db")}),
    )
    .await;
    let (status, t) = api(
        router,
        "POST",
        "templates",
        token,
        json!({"workspace_id":workspace,"title":"Weekly"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{t}");
    Tenant {
        workspace,
        folder,
        note: note_id,
        attachment: a["id"].as_str().unwrap().to_string(),
        database: db["id"].as_str().unwrap().to_string(),
        template: t["id"].as_str().unwrap().to_string(),
    }
}
/// Every route that could reveal or change `t` refuses `token`.
async fn assert_sealed(router: &Router, token: &str, t: &Tenant, own_note: &str) {
    let forbidden =
        |label: &str, status: StatusCode| assert_eq!(status, StatusCode::FORBIDDEN, "{label}");
    let n = &t.note;
    let (_, note) = api(router, "GET", &format!("notes/{n}"), token, json!({})).await;
    for (label, method, path, body) in [
        ("read note", "GET", format!("notes/{n}"), json!({})),
        (
            "save note",
            "PUT",
            format!("notes/{n}"),
            json!({"title":"x","icon":"","blocks":[],"revision":2}),
        ),
        ("history", "GET", format!("notes/{n}/history"), json!({})),
        ("sync read", "POST", format!("notes/{n}/sync"), json!({})),
        ("list shares", "GET", format!("notes/{n}/shares"), json!({})),
        (
            "share",
            "POST",
            format!("notes/{n}/shares"),
            json!({"kind":"link"}),
        ),
        (
            "publish",
            "PUT",
            format!("notes/{n}/publication"),
            json!({"slug":"leak","mode":"live"}),
        ),
        (
            "new note",
            "POST",
            "notes".into(),
            json!({"folder_id":t.folder,"title":"x"}),
        ),
        (
            "new folder",
            "POST",
            "folders".into(),
            json!({"workspace_id":t.workspace,"name":"x"}),
        ),
        (
            "rename folder",
            "PATCH",
            format!("folders/{}", t.folder),
            json!({"name":"x"}),
        ),
        (
            "database",
            "GET",
            format!("databases/{}", t.database),
            json!({}),
        ),
        (
            "new database",
            "POST",
            "databases".into(),
            json!({"folder_id":t.folder,"name":"x"}),
        ),
        (
            "journal",
            "POST",
            "journal".into(),
            json!({"workspace_id":t.workspace,"date":"2026-10-25"}),
        ),
        (
            "journal settings",
            "PUT",
            "journal/settings".into(),
            json!({"workspace_id":t.workspace}),
        ),
        (
            "new template",
            "POST",
            "templates".into(),
            json!({"workspace_id":t.workspace,"title":"x"}),
        ),
        (
            "apply foreign template",
            "POST",
            format!("notes/{own_note}/template"),
            json!({"template_id":t.template,"date":"2026-10-25"}),
        ),
        (
            "journal tasks",
            "POST",
            "journal/tasks".into(),
            json!({"note_id":n,"today":"2026-10-25"}),
        ),
        (
            "mcp credential",
            "POST",
            "mcp-credentials".into(),
            json!({"workspace_id":t.workspace,"name":"x","expires_days":1}),
        ),
        (
            "ai workspace",
            "POST",
            "ai/chats".into(),
            json!({"scope":{"kind":"workspace","id":t.workspace}}),
        ),
        (
            "ai folder",
            "POST",
            "ai/chats".into(),
            json!({"scope":{"kind":"folder","id":t.folder}}),
        ),
        (
            "ai note",
            "POST",
            "ai/chats".into(),
            json!({"scope":{"kind":"note","id":n}}),
        ),
        (
            "grant",
            "PUT",
            "permissions".into(),
            json!({"folder_id":t.folder,"user_id":"x","role":"editor"}),
        ),
        ("admin", "GET", "admin".into(), json!({})),
    ] {
        let (status, _) = api(router, method, &path, token, body).await;
        forbidden(label, status);
    }
    assert!(note.get("blocks").is_none());
    for (label, uri) in [
        ("attachment list", format!("/api/v1/notes/{n}/attachments")),
        (
            "attachment",
            format!("/api/v1/attachments/{}", t.attachment),
        ),
        (
            "attachment preview",
            format!("/api/v1/attachments/{}/preview", t.attachment),
        ),
        ("export", format!("/export/{n}?format=md")),
        ("preview", format!("/preview/{n}")),
    ] {
        let (status, _) = send(router, "GET", &uri, token, Body::empty()).await;
        forbidden(label, status);
    }
    let (status, _) = send(
        router,
        "POST",
        &format!("/api/v1/notes/{n}/attachments?filename=x.txt"),
        token,
        Body::from("x"),
    )
    .await;
    forbidden("upload", status);
}
/// IDs of the workspaces or folders a bootstrap shows, leaving out the user's
/// own personal workspace, which everyone has.
fn visible<'a>(boot: &'a Value, key: &str) -> Vec<&'a str> {
    let personal: Vec<&str> = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["personal"] == true)
        .filter_map(|w| w["id"].as_str())
        .collect();
    boot[key]
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["personal"] != true)
        .filter(|x| {
            !x["workspace_id"]
                .as_str()
                .is_some_and(|w| personal.contains(&w))
        })
        .filter_map(|x| x["id"].as_str())
        .collect()
}
fn ids(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["id"].as_str())
        .collect()
}

#[tokio::test]
async fn workspaces_are_sealed_until_granted_or_shared() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let (_, alice) = user(&router, &root, "Alice", true).await;
    let (_, bob) = user(&router, &root, "Bob", true).await;
    let (carol_id, carol) = user(&router, &root, "Carol", false).await;
    let a = tenant(&router, &alice, "Alpha", "zebra").await;
    let b = tenant(&router, &bob, "Beta", "walrus").await;

    // Without grants or shares, each user sees only their own workspace.
    for (token, own, other, word) in [(&bob, &b, &a, "zebra"), (&alice, &a, &b, "walrus")] {
        let (_, boot) = api(&router, "GET", "bootstrap", token, json!({})).await;
        assert_eq!(visible(&boot, "workspaces"), [own.workspace.as_str()]);
        assert_eq!(visible(&boot, "folders").len(), 2, "General and Templates");
        assert!(!ids(&boot["notes"]).contains(&other.note.as_str()));
        assert_eq!(ids(&boot["databases"]), [own.database.as_str()]);
        assert!(boot["shared_notes"].as_array().unwrap().is_empty());
        assert!(boot["template_folders"].get(&other.workspace).is_none());
        let text = boot.to_string();
        assert!(!text.contains(word) && !text.contains(&other.workspace));
        let (_, found) = api(&router, "POST", "search", token, json!({"query":word})).await;
        assert_eq!(found, json!([]));
        let (_, found) = api(&router, "POST", "search", token, json!({"query":""})).await;
        assert!(!ids(&found).contains(&other.note.as_str()));
        let (_, sharing) = api(&router, "GET", "sharing", token, json!({})).await;
        assert!(!sharing.to_string().contains(word));
        assert_sealed(&router, token, other, &own.note).await;
    }
    // A user without a workspace sees nothing and cannot create one.
    let (_, boot) = api(&router, "GET", "bootstrap", &carol, json!({})).await;
    assert!(
        visible(&boot, "workspaces").is_empty(),
        "only their own My Workspace"
    );
    // Only the guide in their own My Workspace.
    let titles: Vec<&str> = boot["notes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["title"].as_str())
        .collect();
    assert_eq!(titles, ["Getting Started"]);
    let (status, mine) = api(
        &router,
        "POST",
        "workspaces",
        &carol,
        json!({"name":"Mine"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "every member creates workspaces");
    let (status, _) = api(
        &router,
        "DELETE",
        &format!("workspaces/{}", mine["id"].as_str().unwrap()),
        &carol,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_sealed(&router, &carol, &a, &b.note).await;

    // An administrator grant opens exactly that folder, read-only, and nothing else.
    let (status, _) = api(
        &router,
        "PUT",
        "permissions",
        &alice,
        json!({"folder_id":a.folder,"user_id":carol_id,"role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, boot) = api(&router, "GET", "bootstrap", &carol, json!({})).await;
    assert_eq!(visible(&boot, "workspaces"), [a.workspace.as_str()]);
    assert_eq!(
        visible(&boot, "folders"),
        [a.folder.as_str()],
        "the Templates folder is not granted"
    );
    // The granted note, besides the guide in their own workspace.
    let notes: Vec<&str> = boot["notes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["title"] != "Getting Started")
        .filter_map(|n| n["id"].as_str())
        .collect();
    assert_eq!(notes, [a.note.as_str()]);
    let (status, _) = api(
        &router,
        "GET",
        &format!("notes/{}", a.note),
        &carol,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = api(
        &router,
        "GET",
        &format!("notes/{}/history", a.note),
        &carol,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, note) = api(
        &router,
        "GET",
        &format!("notes/{}", a.note),
        &carol,
        json!({}),
    )
    .await;
    let (status, _) = api(&router, "PUT", &format!("notes/{}", a.note), &carol, note).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "viewers cannot edit");
    let (status, _) = api(
        &router,
        "POST",
        &format!("notes/{}/shares", a.note),
        &carol,
        json!({"kind":"link"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "viewers cannot widen the audience"
    );
    assert_sealed(&router, &carol, &b, &a.note).await;

    // A member share opens one note to Bob: not its folder, history, or files elsewhere.
    let (_, second) = api(
        &router,
        "POST",
        "notes",
        &alice,
        json!({"folder_id":a.folder,"title":"zebra second"}),
    )
    .await;
    let (status, _) = api(
        &router,
        "POST",
        &format!("notes/{}/shares", a.note),
        &alice,
        json!({"kind":"member","email":"bob@test.local"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, boot) = api(&router, "GET", "bootstrap", &bob, json!({})).await;
    assert_eq!(visible(&boot, "workspaces"), [b.workspace.as_str()]);
    assert_eq!(ids(&boot["shared_notes"]), [a.note.as_str()]);
    assert!(!boot.to_string().contains("zebra second"));
    let (_, found) = api(&router, "POST", "search", &bob, json!({"query":"zebra"})).await;
    assert_eq!(ids(&found), [a.note.as_str()]);
    let (status, view) = api(
        &router,
        "GET",
        &format!("notes/{}", a.note),
        &bob,
        json!({}),
    )
    .await;
    assert_eq!(
        (status, view["role"].as_str()),
        (StatusCode::OK, Some("viewer"))
    );
    let (status, _) = send(
        &router,
        "GET",
        &format!("/api/v1/attachments/{}", a.attachment),
        &bob,
        Body::empty(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "files of the shared note follow the share"
    );
    let (status, _) = api(
        &router,
        "GET",
        &format!("notes/{}/history", a.note),
        &bob,
        json!({}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "earlier revisions stay with folder members"
    );
    for (label, method, path, body) in [
        (
            "other note",
            "GET",
            format!("notes/{}", second["id"].as_str().unwrap()),
            json!({}),
        ),
        (
            "re-share",
            "POST",
            format!("notes/{}/shares", a.note),
            json!({"kind":"member","email":"carol@test.local"}),
        ),
        (
            "database",
            "GET",
            format!("databases/{}", a.database),
            json!({}),
        ),
        (
            "ai workspace",
            "POST",
            "ai/chats".into(),
            json!({"scope":{"kind":"workspace","id":a.workspace}}),
        ),
        (
            "ai folder",
            "POST",
            "ai/chats".into(),
            json!({"scope":{"kind":"folder","id":a.folder}}),
        ),
    ] {
        let (status, _) = api(&router, method, &path, &bob, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{label}");
    }

    // Workspace access opens every folder of that workspace, including later ones,
    // with edit rights, and nothing of other workspaces.
    let (status, _) = api(
        &router,
        "PUT",
        "workspace-access",
        &alice,
        json!({"workspace_id":a.workspace,"user_id":carol_id,"role":"editor"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "owners share their own workspaces");
    // Administrators cannot open, or share, workspaces of other people.
    let (status, _) = api(
        &router,
        "PUT",
        "workspace-access",
        &admin,
        json!({"workspace_id":a.workspace,"user_id":carol_id,"role":"viewer"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, boot) = api(&router, "GET", "bootstrap", &admin, json!({})).await;
    assert!(!visible(&boot, "workspaces").contains(&a.workspace.as_str()));
    for path in [
        format!("notes/{}", a.note),
        format!("notes/{}/history", a.note),
        format!("databases/{}", a.database),
    ] {
        assert_eq!(
            api(&router, "GET", &path, &admin, json!({})).await.0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    let (status, _) = send(
        &router,
        "GET",
        &format!("/api/v1/attachments/{}", a.attachment),
        &admin,
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "attachment");
    let (_, later) = api(
        &router,
        "POST",
        "folders",
        &alice,
        json!({"workspace_id":a.workspace,"name":"Later"}),
    )
    .await;
    let later = later["id"].as_str().unwrap();
    let (_, boot) = api(&router, "GET", "bootstrap", &carol, json!({})).await;
    let folders = visible(&boot, "folders");
    assert!(folders.contains(&a.folder.as_str()) && folders.contains(&later));
    assert_eq!(folders.len(), 3, "General, Templates, and Later");
    assert!(boot["roles"]
        .as_object()
        .unwrap()
        .values()
        .all(|r| r == "editor"));
    assert!(ids(&boot["notes"]).contains(&second["id"].as_str().unwrap()));
    let (_, note) = api(
        &router,
        "GET",
        &format!("notes/{}", a.note),
        &carol,
        json!({}),
    )
    .await;
    assert_eq!(note["role"], "editor");
    let (status, _) = api(&router, "PUT", &format!("notes/{}", a.note), &carol, note).await;
    assert_eq!(status, StatusCode::OK, "workspace members edit notes");
    let (status, _) = api(
        &router,
        "POST",
        "notes",
        &carol,
        json!({"folder_id":later,"title":"By Carol"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = api(
        &router,
        "POST",
        "folders",
        &carol,
        json!({"workspace_id":a.workspace,"name":"x"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "workspace editors add folders");
    assert_sealed(&router, &carol, &b, &a.note).await;
    let (_, admin_view) = api(&router, "GET", "admin", &root, json!({})).await;
    // Who has access to Alice's workspace is hers to know.
    assert_eq!(admin_view["workspace_grants"], json!([]));
    api(
        &router,
        "PUT",
        "workspace-access",
        &alice,
        json!({"workspace_id":a.workspace,"user_id":carol_id,"role":"viewer"}),
    )
    .await;
    let (_, note) = api(
        &router,
        "GET",
        &format!("notes/{}", a.note),
        &carol,
        json!({}),
    )
    .await;
    let (status, _) = api(&router, "PUT", &format!("notes/{}", a.note), &carol, note).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "workspace viewers read only");
    api(
        &router,
        "PUT",
        "workspace-access",
        &alice,
        json!({"workspace_id":a.workspace,"user_id":carol_id,"role":"none"}),
    )
    .await;

    // Revoking the grant and the share restores full separation.
    let (_, shares) = api(
        &router,
        "GET",
        &format!("notes/{}/shares", a.note),
        &alice,
        json!({}),
    )
    .await;
    for share in shares["shares"].as_array().unwrap() {
        let path = format!("notes/{}/shares/{}", a.note, share["id"].as_str().unwrap());
        let (status, _) = api(&router, "DELETE", &path, &alice, json!({})).await;
        assert_eq!(status, StatusCode::OK);
    }
    api(
        &router,
        "PUT",
        "permissions",
        &alice,
        json!({"folder_id":a.folder,"user_id":carol_id,"role":"none"}),
    )
    .await;
    assert_sealed(&router, &bob, &a, &b.note).await;
    assert_sealed(&router, &carol, &a, &b.note).await;
}
