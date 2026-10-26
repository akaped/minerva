//! PDF templates (plugins/pdf-prints) and note metadata used as template variables.
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
    body: Value,
) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}
async fn api(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let (status, text) = send(router, method, &format!("/api/v1/{path}"), token, body).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}
async fn get(router: &Router, uri: &str, token: &str) -> String {
    let (status, text) = send(router, "GET", uri, token, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {text}");
    text
}

#[tokio::test]
async fn templates_fill_from_note_metadata() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let (_, login) = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await;
    let root = login["token"].as_str().unwrap().to_string();
    let admin = common::member(&router, &root, "owner@test.local").await;
    let (_, boot) = api(&router, "GET", "bootstrap", &admin, json!({})).await;
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Twenty built-in templates, in order, with their declared variables.
    let index: Value =
        serde_json::from_str(&get(&router, "/plugins/index.json", "").await).unwrap();
    let prints = index["pdf_prints"].as_array().unwrap();
    assert_eq!(prints.len(), 20);
    assert_eq!(prints[0]["id"], "01-standard");
    assert_eq!(prints[1]["name"], "Company Report");
    assert!(prints[1]["variables"]
        .as_array()
        .unwrap()
        .contains(&json!("classification")));
    assert_eq!(prints[17]["orientation"], "landscape");
    assert!(prints
        .iter()
        .all(|p| !p["users"].as_str().unwrap().is_empty()));

    // A note with metadata and placeholders in its text.
    let (_, mut n) = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Quarterly Security Review"}),
    )
    .await;
    let nid = n["id"].as_str().unwrap().to_string();
    n["blocks"] = json!([
        {"id":"a","markdown":"Prepared by {{author}} for {{Company}}. Unknown: {{nothing}}."},
        {"id":"b","markdown":"## Findings\nSee `{{author}}` in code. #critical"},
        {"id":"c","markdown":"### Detail #high\nText."}
    ]);
    api(&router, "PUT", &format!("notes/{nid}"), &admin, n).await;
    let (status, _) = api(
        &router,
        "PUT",
        &format!("notes/{nid}/metadata"),
        &admin,
        json!({"metadata":{"1st":"x"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "keys start with a letter");
    let (status, _) = api(
        &router,
        "PUT",
        &format!("notes/{nid}/metadata"),
        &admin,
        json!({"metadata":{},"print_template":"missing"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, saved) = api(&router, "PUT", &format!("notes/{nid}/metadata"), &admin, json!({
        "print_template": "02-company-report",
        "metadata": {"Author": "Security Team", "Company Name": "Acme <Corp>", "company": "Acme Corporation", "version": "1.0", "classification": "Confidential", "date": "2026-10-25", "empty": ""}
    })).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["metadata"]["company_name"], "Acme <Corp>",
        "keys are normalized"
    );
    assert!(
        saved["metadata"].get("empty").is_none(),
        "empty values are dropped"
    );

    // Editing the text keeps the metadata.
    let (_, mut current) = api(&router, "GET", &format!("notes/{nid}"), &admin, json!({})).await;
    assert_eq!(current["print_template"], "02-company-report");
    current["title"] = json!("Quarterly Security Review");
    current.as_object_mut().unwrap().remove("metadata");
    api(&router, "PUT", &format!("notes/{nid}"), &admin, current).await;
    let (_, current) = api(&router, "GET", &format!("notes/{nid}"), &admin, json!({})).await;
    assert_eq!(current["metadata"]["author"], "Security Team");

    // The company report: cover, header and footer, contents, escaping, placeholders.
    let html = get(
        &router,
        &format!("/export/{nid}?format=print&template=02-company-report"),
        &admin,
    )
    .await;
    assert!(html.contains("class=\"export pdf pdf-02-company-report\""));
    assert!(html.contains("<h1 class=\"pdf-title\">Quarterly Security Review</h1>"));
    assert!(html.contains("Acme Corporation") && html.contains("Confidential"));
    assert!(
        html.contains("<span class=\"pdf-placeholder\">&#123;&#123;department&#125;&#125;</span>"),
        "missing values print placeholders"
    );
    assert!(html
        .contains("<span class=\"pdf-placeholder pdf-logo-placeholder\">Acme Corporation</span>"));
    assert!(
        html.contains("<tr><th>Company name</th><td>Acme &lt;Corp&gt;</td></tr>")
            && !html.contains("<dt>Company name</dt>"),
        "extra metadata follows the template's fields in the table, not on the cover"
    );
    assert!(
        html.contains("<h2 id=\"sec-1\">Findings</h2>")
            && html.contains("<a href=\"#sec-2\">Detail</a>")
    );
    assert!(html.contains("Prepared by Security Team for {{Company}}. Unknown: {{nothing}}."));
    assert!(
        html.contains("<code>{{author}}</code>"),
        "code stays literal"
    );
    assert!(html.contains("data-tag=\"critical\""));
    assert!(html.contains("counter(pages)") && html.contains("size: A4;"));
    assert!(!html.contains("{{#") && !html.contains("{{{"));

    // Every template renders cleanly; landscape and the resume layout apply.
    for p in prints {
        let id = p["id"].as_str().unwrap();
        let html = get(
            &router,
            &format!("/export/{nid}?format=html&template={id}"),
            &admin,
        )
        .await;
        assert!(html.contains(&format!("pdf-{id}")), "{id}");
        assert!(
            !html.contains("{{#") && !html.contains("{{^") && !html.contains("{{{"),
            "{id} left a placeholder"
        );
        assert!(!html.contains("<script"), "{id}");
    }
    let landscape = get(
        &router,
        &format!("/export/{nid}?format=print&size=letter&template=18-presentation-handout"),
        &admin,
    )
    .await;
    assert!(landscape.contains("size: letter landscape;"));
    let resume = get(
        &router,
        &format!("/export/{nid}?format=print&template=16-resume"),
        &admin,
    )
    .await;
    assert!(
        resume.contains("<h1>Quarterly Security Review</h1>")
            && resume
                .contains("<span class=\"pdf-placeholder\">&#123;&#123;role&#125;&#125;</span>")
    );
    // An unknown or missing template uses Standard; there is no unstyled export.
    for query in ["template=nope", "template="] {
        let standard = get(
            &router,
            &format!("/export/{nid}?format=print&{query}"),
            &admin,
        )
        .await;
        assert!(standard.contains("<body class=\"export pdf pdf-01-standard\">"));
        assert!(standard.contains("Prepared by Security Team"));
    }

    // Markdown: metadata in front matter, placeholders filled.
    let md = get(&router, &format!("/export/{nid}?format=md"), &admin).await;
    assert!(
        md.contains("company: \"Acme Corporation\"")
            && md.contains("classification: \"Confidential\"")
    );
    assert!(md.contains("Prepared by Security Team"));

    // Notes filled from a template inherit its metadata and PDF template.
    let wid = common::team_workspace(&boot)["id"].as_str().unwrap();
    let (_, template) = api(
        &router,
        "POST",
        "templates",
        &admin,
        json!({"workspace_id":wid,"title":"Minutes"}),
    )
    .await;
    let tid = template["id"].as_str().unwrap();
    api(&router, "PUT", &format!("notes/{tid}/metadata"), &admin, json!({"print_template":"03-meeting-minutes","metadata":{"date":"{{date}}","location":"Room 4"}})).await;
    let (_, target) = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Weekly sync"}),
    )
    .await;
    let (status, filled) = api(
        &router,
        "POST",
        &format!("notes/{}/template", target["id"].as_str().unwrap()),
        &admin,
        json!({"template_id":tid,"date":"2026-10-25"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{filled}");
    assert_eq!(filled["print_template"], "03-meeting-minutes");
    assert_eq!(filled["metadata"]["date"], "2026-10-25");
    assert_eq!(filled["metadata"]["location"], "Room 4");
}
