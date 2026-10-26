//! Note export formats: Markdown, Markdown + files (.zip), standalone HTML, print page.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use std::io::Read;
use tower::ServiceExt;
mod common;

const PASSWORD: &str = "test-password-1234";

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}
async fn send(router: &Router, method: &str, uri: &str, token: &str, body: Vec<u8>) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("x-minerva-client", "1");
    if !token.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    Reply {
        status: response.status(),
        headers: response.headers().clone(),
        body: response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    }
}
async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> Value {
    let reply = send(
        router,
        method,
        &format!("/api/v1/{path}"),
        token,
        body.to_string().into_bytes(),
    )
    .await;
    serde_json::from_slice(&reply.body).unwrap_or(Value::Null)
}
fn text(reply: &Reply) -> String {
    String::from_utf8_lossy(&reply.body).into_owned()
}

#[tokio::test]
async fn exports_markdown_zip_html_and_print_with_permissions() {
    let router = app(Store::open(":memory:", "admin@test.local", PASSWORD).unwrap());
    let admin = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let root = admin;
    let admin = common::member(&router, &root, "owner@test.local").await;
    let boot = api(&router, "GET", "bootstrap", &admin, json!({})).await;
    let workspace = common::team_workspace(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let folder = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let note = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Trip: plan/2027"}),
    )
    .await;
    let nid = note["id"].as_str().unwrap().to_string();
    let target = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder,"title":"Packing list"}),
    )
    .await;
    let png = b"\x89PNG\r\n\x1a\nchart-bytes".to_vec();
    let file = serde_json::from_slice::<Value>(
        &send(
            &router,
            "POST",
            &format!("/api/v1/notes/{nid}/attachments?filename=chart.png"),
            &admin,
            png.clone(),
        )
        .await
        .body,
    )
    .unwrap();
    let fid = file["id"].as_str().unwrap().to_string();
    // A file from a folder the reader cannot see, referenced by the note.
    let hidden_folder = api(
        &router,
        "POST",
        "folders",
        &admin,
        json!({"workspace_id":workspace,"name":"Private"}),
    )
    .await;
    let hidden_note = api(
        &router,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":hidden_folder["id"],"title":"Private"}),
    )
    .await;
    let secret = serde_json::from_slice::<Value>(
        &send(
            &router,
            "POST",
            &format!(
                "/api/v1/notes/{}/attachments?filename=secret.txt",
                hidden_note["id"].as_str().unwrap()
            ),
            &admin,
            b"top secret".to_vec(),
        )
        .await
        .body,
    )
    .unwrap();
    let sid = secret["id"].as_str().unwrap().to_string();

    let mut save = note.clone();
    save["tags"] = json!(["travel"]);
    save["icon"] = json!("🧳");
    save["cover"] = json!(format!("/api/v1/attachments/{fid}"));
    save["blocks"] = json!([
        {"id":"a","markdown":"## Days"},
        {"id":"b","markdown":"- [ ] Book flights"},
        {"id":"c","markdown":"- [x] Renew passport"},
        {"id":"d","markdown":format!("See [Packing](#note/{}) and ![Chart](/api/v1/attachments/{fid}/preview)", target["id"].as_str().unwrap())},
        {"id":"e","markdown":format!("[Secret](/api/v1/attachments/{sid}) and $E = mc^2$ with ==care==")},
        {"id":"f","markdown":"```rust\nfn main() {}\n```"},
    ]);
    api(&router, "PUT", &format!("notes/{nid}"), &admin, save).await;

    // Markdown
    let md = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=md"),
        &admin,
        vec![],
    )
    .await;
    assert_eq!(md.status, StatusCode::OK);
    assert!(md.headers["content-disposition"]
        .to_str()
        .unwrap()
        .contains("Trip_%20plan_2027.md"));
    let body = text(&md);
    assert!(body.starts_with("---\ntitle: \"Trip: plan/2027\"\ntags: [\"travel\"]\nicon: \"🧳\"\ncover: \"http://localhost/api/v1/attachments/"), "{body}");
    assert!(
        body.contains(
            "# Trip: plan/2027\n\n## Days\n\n- [ ] Book flights\n- [x] Renew passport\n\n"
        ),
        "{body}"
    );
    assert!(body.contains("See [[Packing list]] and ![Chart](http://localhost/api/v1/attachments/"));
    let plain = text(
        &send(
            &router,
            "GET",
            &format!("/export/{nid}?format=md&front_matter=0"),
            &admin,
            vec![],
        )
        .await,
    );
    assert!(plain.starts_with("# Trip: plan/2027"));

    // Zip: the note plus readable referenced files, links rewritten.
    let zip = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=zip"),
        &admin,
        vec![],
    )
    .await;
    assert_eq!(zip.headers["content-type"], "application/zip");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip.body)).unwrap();
    let names: Vec<String> = archive
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect();
    assert!(
        names.contains(&"Trip_ plan_2027.md".to_string()),
        "{names:?}"
    );
    assert!(names.contains(&"files/chart.png".to_string()));
    let mut chart = vec![];
    archive
        .by_name("files/chart.png")
        .unwrap()
        .read_to_end(&mut chart)
        .unwrap();
    assert_eq!(chart, png);
    let mut inner = String::new();
    archive
        .by_name("Trip_ plan_2027.md")
        .unwrap()
        .read_to_string(&mut inner)
        .unwrap();
    assert!(
        inner.contains("![Chart](files/chart.png)") && inner.contains("cover: \"files/chart.png\""),
        "{inner}"
    );

    // Standalone HTML: styles inline, images embedded, no scripts.
    let html = text(
        &send(
            &router,
            "GET",
            &format!("/export/{nid}?format=html"),
            &admin,
            vec![],
        )
        .await,
    );
    assert!(html.contains("data:image/png;base64,") && html.contains("<style>"));
    assert!(!html.contains("<script") && !html.contains("href=\"/public.css\""));
    assert!(html.contains("<math") && html.contains("<mark>care</mark>") && html.contains("hl-"));
    assert!(html.contains("<span class=\"tag\">#travel</span>"));

    // Print page: framed by the app only, page size selectable.
    let print = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=print&size=letter&cover=0"),
        &admin,
        vec![],
    )
    .await;
    let page = text(&print);
    assert!(print.headers["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'self'"));
    assert!(
        page.contains("<script src=\"/export.js\" defer></script>")
            && page.contains("size: letter")
    );
    assert!(!page.contains("<div class=\"export-cover"));
    assert!(
        page.contains("<style>") && !page.contains("stylesheet"),
        "styles are embedded"
    );
    let app_page = send(&router, "GET", "/", "", vec![]).await;
    assert!(app_page.headers["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'none'"));

    // Permissions: sign-in required; readers export what they can read only.
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/export/{nid}?format=md"),
            "",
            vec![]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let user = api(
        &router,
        "POST",
        "users",
        &root,
        json!({"name":"Reader","email":"reader@test.local","password":PASSWORD}),
    )
    .await;
    let reader = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"reader@test.local","password":PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/export/{nid}?format=md"),
            &reader,
            vec![]
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    api(
        &router,
        "PUT",
        "permissions",
        &admin,
        json!({"folder_id":folder,"user_id":user["id"],"role":"viewer"}),
    )
    .await;
    let zip = send(
        &router,
        "GET",
        &format!("/export/{nid}?format=zip"),
        &reader,
        vec![],
    )
    .await;
    assert_eq!(zip.status, StatusCode::OK);
    let archive = zip::ZipArchive::new(std::io::Cursor::new(zip.body)).unwrap();
    assert!(
        !archive.file_names().any(|n| n.unwrap().contains("secret")),
        "files the reader cannot open are excluded"
    );
    assert_eq!(
        send(
            &router,
            "GET",
            &format!("/export/{nid}?format=docx"),
            &reader,
            vec![]
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
}
