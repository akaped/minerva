//! Journals, templates, and the unfinished journal tasks list.
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
async fn login(router: &Router, email: &str) -> String {
    api(
        router,
        "POST",
        "login",
        "",
        json!({"email":email,"password":PASSWORD}),
    )
    .await
    .1["token"]
        .as_str()
        .unwrap()
        .into()
}
async fn write(router: &Router, token: &str, note: &Value, blocks: &[&str]) -> Value {
    let mut save = note.clone();
    save["blocks"] = json!(blocks
        .iter()
        .enumerate()
        .map(|(i, m)| json!({"id":format!("b{i}"),"markdown":m}))
        .collect::<Vec<_>>());
    let (status, saved) = api(
        router,
        "PUT",
        &format!("notes/{}", note["id"].as_str().unwrap()),
        token,
        save,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    saved
}

#[tokio::test]
async fn journals_templates_and_unfinished_tasks() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let root = login(&router, "admin@test.local").await;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let boot = api(&router, "GET", "bootstrap", &admin, json!({})).await.1;
    let ws = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let general = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, user) = api(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Member","email":"member@test.local","password":PASSWORD}),
    )
    .await;
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":general,"user_id":user["id"],"role":"editor"}),
    )
    .await;
    let member = login(&router, "member@test.local").await;

    // Editing one folder is not enough to set up the Journal and Templates
    // folders: that takes editing the whole workspace.
    let (status, error) = api(
        &router,
        "POST",
        "journal",
        &member,
        json!({"workspace_id":ws,"date":"2026-10-20"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{error}");
    assert!(error["error"]
        .as_str()
        .unwrap()
        .contains("can edit this workspace"));
    assert_eq!(
        api(
            &router,
            "POST",
            "templates",
            &member,
            json!({"workspace_id":ws,"title":"T"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            &router,
            "POST",
            "journal",
            &admin,
            json!({"workspace_id":ws,"date":"2026-02-30"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // Journal entries: one note per date, reopened rather than duplicated.
    let (status, day) = api(
        &router,
        "POST",
        "journal",
        &admin,
        json!({"workspace_id":ws,"date":"2026-10-20"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{day}");
    assert_eq!(day["created"], true);
    assert_eq!(day["journal"], "2026-10-20");
    assert_eq!(day["title"], "2026-10-20");
    let again = api(
        &router,
        "POST",
        "journal",
        &admin,
        json!({"workspace_id":ws,"date":"2026-10-20"}),
    )
    .await
    .1;
    assert_eq!(again["id"], day["id"]);
    assert_eq!(again["created"], false);
    let boot = api(&router, "GET", "bootstrap", &admin, json!({})).await.1;
    let journal_folder = boot["journal_folders"][&ws].as_str().unwrap().to_string();
    assert_eq!(day["folder_id"], journal_folder.as_str());
    assert!(boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == journal_folder.as_str() && f["name"] == "Journal"));
    // A member without access to the Journal folder cannot open entries.
    assert_eq!(
        api(
            &router,
            "POST",
            "journal",
            &member,
            json!({"workspace_id":ws,"date":"2026-10-20"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // Templates: variables are filled; empty notes only; only template notes apply.
    let (_, template) = api(
        &router,
        "POST",
        "templates",
        &admin,
        json!({"workspace_id":ws,"title":"Daily"}),
    )
    .await;
    let template = write(
        &router,
        &admin,
        &template,
        &[
            "## {{long_date}}",
            "Plan for {{weekday}} {{date}} in {{title}}",
            "[[uncompleted_journal_tasks]]",
        ],
    )
    .await;
    let boot = api(&router, "GET", "bootstrap", &admin, json!({})).await.1;
    assert_eq!(boot["template_folders"][&ws], template["folder_id"]);
    let blank = api(
        &router,
        "POST",
        "notes",
        &member,
        json!({"folder_id":general,"title":"Retro"}),
    )
    .await
    .1;
    let (status, filled) = api(
        &router,
        "POST",
        &format!("notes/{}/template", blank["id"].as_str().unwrap()),
        &member,
        json!({"template_id":template["id"],"date":"2026-03-05"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "members need read access to the Templates folder: {filled}"
    );
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":template["folder_id"],"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let (status, filled) = api(
        &router,
        "POST",
        &format!("notes/{}/template", blank["id"].as_str().unwrap()),
        &member,
        json!({"template_id":template["id"],"date":"2026-03-05"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filled}");
    let texts: Vec<&str> = filled["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["markdown"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        vec![
            "## Thursday, March 5, 2026",
            "Plan for Thursday 2026-03-05 in Retro",
            "[[uncompleted_journal_tasks]]"
        ]
    );
    assert_ne!(
        filled["blocks"][0]["id"], template["blocks"][0]["id"],
        "copies get new block IDs"
    );
    assert!(filled["rendered"][2]
        .as_str()
        .unwrap()
        .contains("journal-tasks"));
    assert_eq!(
        api(
            &router,
            "POST",
            &format!("notes/{}/template", blank["id"].as_str().unwrap()),
            &member,
            json!({"template_id":template["id"],"date":"2026-03-05"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let other = api(
        &router,
        "POST",
        "notes",
        &member,
        json!({"folder_id":general,"title":"Other"}),
    )
    .await
    .1;
    assert_eq!(
        api(
            &router,
            "POST",
            &format!("notes/{}/template", other["id"].as_str().unwrap()),
            &member,
            json!({"template_id":blank["id"],"date":"2026-03-05"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // New journal days use the configured template, with the entry's date.
    assert_eq!(
        api(
            &router,
            "PUT",
            "journal/settings",
            &member,
            json!({"workspace_id":ws,"template_id":template["id"]})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            &router,
            "PUT",
            "journal/settings",
            &admin,
            json!({"workspace_id":ws,"template_id":blank["id"]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    api(
        &router,
        "PUT",
        "journal/settings",
        &admin,
        json!({"workspace_id":ws,"template_id":template["id"]}),
    )
    .await;
    let templated = api(
        &router,
        "POST",
        "journal",
        &admin,
        json!({"workspace_id":ws,"date":"2026-10-24"}),
    )
    .await
    .1;
    assert_eq!(
        templated["blocks"][0]["markdown"],
        "## Saturday, October 24, 2026"
    );
    assert_eq!(
        templated["blocks"][1]["markdown"],
        "Plan for Saturday 2026-10-24 in 2026-10-24"
    );

    // Unfinished tasks from earlier entries.
    write(
        &router,
        &admin,
        &day,
        &["- [ ] Call Ana\n- [ ] Buy milk", "- [ ] Water plants"],
    )
    .await;
    let d21 = api(
        &router,
        "POST",
        "journal",
        &admin,
        json!({"workspace_id":ws,"date":"2026-10-21"}),
    )
    .await
    .1;
    let d21 = write(
        &router,
        &admin,
        &d21,
        &[
            "- [x] buy   MILK",
            "- [ ] call ana",
            "- [ ] Write **report**",
        ],
    )
    .await;
    let d22 = api(
        &router,
        "POST",
        "journal",
        &admin,
        json!({"workspace_id":ws,"date":"2026-10-22"}),
    )
    .await
    .1;
    let d22 = write(
        &router,
        &admin,
        &d22,
        &["[[uncompleted_journal_tasks]]", "- [ ] Water plants"],
    )
    .await;
    let (status, tasks) = api(
        &router,
        "POST",
        "journal/tasks",
        &admin,
        json!({"note_id":d22["id"],"today":"2026-10-25"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tasks}");
    let texts: Vec<&str> = tasks
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        vec!["Call Ana", "Write **report**"],
        "completed later, duplicated, and already-present tasks are excluded"
    );
    assert_eq!(
        tasks[0]["date"], "2026-10-20",
        "a repeated task keeps its earliest date"
    );
    assert_eq!(tasks[1]["html"], "Write <strong>report</strong>");
    // Entries on or after the context entry's date are not included.
    let d23 = api(
        &router,
        "POST",
        "journal",
        &admin,
        json!({"workspace_id":ws,"date":"2026-10-23"}),
    )
    .await
    .1;
    write(&router, &admin, &d23, &["- [ ] Future task"]).await;
    let tasks = api(
        &router,
        "POST",
        "journal/tasks",
        &admin,
        json!({"note_id":d22["id"],"today":"2026-10-25"}),
    )
    .await
    .1;
    assert!(!tasks.to_string().contains("Future task"));
    // A non-journal note gathers entries up to today.
    let tasks = api(
        &router,
        "POST",
        "journal/tasks",
        &admin,
        json!({"note_id":other["id"],"today":"2026-10-23"}),
    )
    .await
    .1;
    assert!(
        tasks.to_string().contains("Future task") && tasks.to_string().contains("Water plants")
    );

    // Completing a task marks every open occurrence in earlier entries.
    let (_, done) = api(
        &router,
        "POST",
        "journal/tasks/complete",
        &admin,
        json!({"note_id":d22["id"],"today":"2026-10-25","text":"Call Ana"}),
    )
    .await;
    assert_eq!(done["completed"], 2);
    let tasks = api(
        &router,
        "POST",
        "journal/tasks",
        &admin,
        json!({"note_id":d22["id"],"today":"2026-10-25"}),
    )
    .await
    .1;
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    let first = api(
        &router,
        "GET",
        &format!("notes/{}", day["id"].as_str().unwrap()),
        &admin,
        json!({}),
    )
    .await
    .1;
    assert!(first["blocks"][0]["markdown"]
        .as_str()
        .unwrap()
        .starts_with("- [x] Call Ana\n- [ ] Buy milk"));
    let second = api(
        &router,
        "GET",
        &format!("notes/{}", d21["id"].as_str().unwrap()),
        &admin,
        json!({}),
    )
    .await
    .1;
    assert_eq!(second["blocks"][1]["markdown"], "- [x] call ana");

    // Readers only see tasks from entries they may open.
    let tasks = api(
        &router,
        "POST",
        "journal/tasks",
        &member,
        json!({"note_id":other["id"],"today":"2026-10-25"}),
    )
    .await
    .1;
    assert_eq!(tasks, json!([]));
}
