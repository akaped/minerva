//! Importing from Notion, Obsidian, Logseq and Evernote: pages become notes in
//! folders like the export's, with links, tags, attachments and databases.
use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use std::io::Write;
use tower::ServiceExt;
mod common;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\nIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n-\xb4\0\0\0\0IEND\xaeB`\x82";
const NOTION: &str = "0123456789abcdef0123456789abcdef";
const SUB: &str = "fedcba9876543210fedcba9876543210";

async fn api(router: &Router, method: &str, path: &str, token: &str, body: Value) -> Value {
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
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}
async fn import(
    router: &Router,
    token: &str,
    folder: &str,
    name: &str,
    bytes: Vec<u8>,
) -> (u16, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/import?folder_id={folder}&name={name}"))
                .header("x-minerva-client", "1")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(bytes))
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
fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, data) in files {
        archive
            .start_file(*path, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(data).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
/// Notes, by title, with their folder's name and their text.
async fn notes(router: &Router, token: &str) -> std::collections::HashMap<String, (String, Value)> {
    let boot = api(router, "GET", "bootstrap", token, json!({})).await;
    let mut out = std::collections::HashMap::new();
    for n in boot["notes"].as_array().unwrap() {
        let folder = boot["folders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == n["folder_id"])
            .map(|f| f["name"].as_str().unwrap().to_string())
            .unwrap_or_default();
        let full = api(
            router,
            "GET",
            &format!("notes/{}", n["id"].as_str().unwrap()),
            token,
            json!({}),
        )
        .await;
        out.insert(n["title"].as_str().unwrap().to_string(), (folder, full));
    }
    out
}
fn text(note: &Value) -> String {
    note["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["markdown"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn imports_from_notion_obsidian_logseq_and_evernote() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let root = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let token = common::member(&router, &root, "owner@test.local").await;
    let boot = api(&router, "GET", "bootstrap", &token, json!({})).await;
    let target = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Notion: a page with a subpage and a picture, and a table (written twice).
    let plans = format!("# Plans\n\nSee [Sub](Plans%20{NOTION}/Sub%20{SUB}.md) and:\n\n![](Plans%20{NOTION}/pic.png)\n\n- step one\n    - detail");
    let csv = "\u{feff}Name,Status,Due,Done,Points\nWrite,Doing,\"November 1, 2026\",Yes,3\nShip,Done,\"November 9, 2026\",No,5\nTest,Done,,No,1\n";
    let notion = zip(&[
        (&format!("Plans {NOTION}.md"), plans.as_bytes()),
        (&format!("Plans {NOTION}/Sub {SUB}.md"), b"# Sub\n\nInside."),
        (&format!("Plans {NOTION}/pic.png"), PNG),
        (&format!("Tasks {SUB}.csv"), b"Name\nWrite\n"),
        (&format!("Tasks {SUB}_all.csv"), csv.as_bytes()),
    ]);
    // Notion nests its export in a second zip.
    let (status, report) = import(
        &router,
        &token,
        &target,
        "notion.zip",
        zip(&[("Export-1.zip", &notion)]),
    )
    .await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(
        (
            report["notes"].clone(),
            report["databases"].clone(),
            report["attachments"].clone()
        ),
        (json!(2), json!(1), json!(1)),
        "{report}"
    );
    let all = notes(&router, &token).await;
    let (_, plans) = &all["Plans"];
    let (folder, sub) = &all["Sub"];
    assert_eq!(
        folder, "Plans",
        "subpages sit in a folder named after their page"
    );
    let body = text(plans);
    assert!(
        body.contains(&format!("[Sub](#note/{})", sub["id"].as_str().unwrap())),
        "{body}"
    );
    assert!(body.contains("![pic.png](/api/v1/attachments/"), "{body}");
    assert!(
        body.contains("- step one\n  - detail"),
        "list items are nested blocks: {body}"
    );
    let boot = api(&router, "GET", "bootstrap", &token, json!({})).await;
    let db_id = boot["databases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "Tasks")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let db = api(
        &router,
        "GET",
        &format!("databases/{db_id}"),
        &token,
        json!({}),
    )
    .await;
    let kinds: Vec<_> = db["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        ["text", "select", "date", "checkbox", "number"],
        "{db}"
    );
    assert_eq!(
        db["rows"].as_array().unwrap().len(),
        3,
        "the complete table is used"
    );

    // Obsidian: front matter, wiki links with aliases, embeds found by file name,
    // and the app's own folder left out.
    let vault = zip(&[
        ("Vault/Note A.md", b"---\ntags: [physics, ideas]\nauthor: Ada\n---\nLinks to [[Note B|bee]] and [[Missing]].\n![[img.png]]" as &[u8]),
        ("Vault/sub/Note B.md", b"Back to [[Note A]]."),
        ("Vault/attachments/img.png", PNG),
        ("Vault/.obsidian/app.json", b"{}"),
    ]);
    let (status, report) = import(&router, &token, &target, "vault.zip", vault).await;
    assert_eq!(
        (status, report["notes"].clone()),
        (200, json!(2)),
        "{report}"
    );
    let all = notes(&router, &token).await;
    let (_, a) = &all["Note A"];
    let (folder, b) = &all["Note B"];
    assert_eq!(folder, "sub");
    assert_eq!(a["tags"], json!(["physics", "ideas"]));
    assert_eq!(a["metadata"]["author"], "Ada");
    let body = text(a);
    assert!(
        body.contains(&format!("[bee](#note/{})", b["id"].as_str().unwrap())),
        "{body}"
    );
    assert!(
        body.contains("and Missing."),
        "unknown pages stay as text: {body}"
    );
    assert!(body.contains("![img.png](/api/v1/attachments/"), "{body}");

    // Logseq: namespaced pages, journals, page properties and block references.
    let graph = zip(&[
        ("graph/pages/books___dune.md", b"tags:: scifi\n\n- Spice\n  id:: 650e1a2b-0000-4000-8000-000000000001\n- Sand\n\t- Worms" as &[u8]),
        ("graph/journals/2026_11_01.md", b"- Read ((650e1a2b-0000-4000-8000-000000000001)) in [[books/dune]]\n- {{embed ((650e1a2b-0000-4000-8000-000000000001))}}\n- {{embed [[books/dune]]}}"),
        ("graph/logseq/config.edn", b"{}"),
    ]);
    let (status, report) = import(&router, &token, &target, "graph.zip", graph).await;
    assert_eq!(
        (status, report["notes"].clone()),
        (200, json!(2)),
        "{report}"
    );
    let all = notes(&router, &token).await;
    let (folder, dune) = &all["books/dune"];
    assert_eq!(folder, "pages");
    assert_eq!(dune["tags"], json!(["scifi"]));
    assert_eq!(
        text(dune),
        "- Spice\n- Sand\n  - Worms",
        "block IDs are Logseq's own"
    );
    // The referenced block keeps Logseq's ID, so a reference reaches it.
    assert_eq!(
        dune["blocks"][0]["id"],
        "650e1a2b-0000-4000-8000-000000000001"
    );
    let (_, day) = &all["2026-11-01"];
    let note = dune["id"].as_str().unwrap();
    let block = "650e1a2b-0000-4000-8000-000000000001";
    assert_eq!(
        text(day),
        format!("- Read [Spice](#note/{note}#{block}) in [books/dune](#note/{note})\n- ![Spice](#note/{note}#{block})\n- ![books/dune](#note/{note})")
    );

    // Evernote: notes, tags, checklists and embedded files.
    let enex = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><en-export><note><title>Trip &amp; plan</title><content><![CDATA[<en-note><div><en-todo checked="false"/>Pack</div><div><en-media hash="{hash}" type="image/png"/></div></en-note>]]></content><created>20261101T090000Z</created><tag>travel</tag><resource><data encoding="base64">{data}</data><mime>image/png</mime><resource-attributes><file-name>map.png</file-name></resource-attributes></resource></note></en-export>"#,
        hash = "93ca32a536da1698ea979f183679af29",
        data = base64_png(),
    );
    let (status, report) = import(&router, &token, &target, "notes.enex", enex.into_bytes()).await;
    assert_eq!(status, 200, "{report}");
    let all = notes(&router, &token).await;
    let (_, trip) = &all["Trip & plan"];
    assert_eq!(trip["tags"], json!(["travel"]));
    assert_eq!(trip["metadata"]["created"], "2026-11-01");
    let body = text(trip);
    assert!(body.starts_with("- [ ] Pack"), "{body}");
    assert!(body.contains("![map.png](/api/v1/attachments/"), "{body}");

    // Nothing importable, and folders one cannot edit.
    let (status, _) = import(
        &router,
        &token,
        &target,
        "empty.zip",
        zip(&[("a.png", PNG)]),
    )
    .await;
    assert_eq!(status, 400);
    let other = common::member(&router, &root, "other@test.local").await;
    let (status, _) = import(&router, &other, &target, "x.md", b"Hello".to_vec()).await;
    assert_eq!(status, 403);
}

fn base64_png() -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.encode(PNG)
}

#[tokio::test]
async fn notion_multi_select_and_relations_become_real_columns() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let root = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let token = common::member(&router, &root, "owner@test.local").await;
    let boot = api(&router, "GET", "bootstrap", &token, json!({})).await;
    let target = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let id1 = "0123456789abcdef0123456789abcdef";
    let id2 = "fedcba9876543210fedcba9876543210";
    let people = "Name,Team\nAda,\"Design, Research\"\nBo,Research\nCy,\"Design, Ops\"\n";
    let projects = format!(
        "Name,Owners\nLaunch,\"Ada (People%20{id1}/Ada%20{id2}.md), Bo (People%20{id1}/Bo%20{id2}.md)\"\nAudit,\"Cy (People%20{id1}/Cy%20{id2}.md)\"\nOrphan,\"Zed (Elsewhere%20{id1}/Zed%20{id2}.md)\"\n"
    );
    let export = zip(&[
        (&format!("People {id1}.csv"), people.as_bytes()),
        (&format!("Projects {id2}.csv"), projects.as_bytes()),
    ]);
    let (status, report) = import(&router, &token, &target, "notion.zip", export).await;
    assert_eq!(status, 200, "{report}");
    assert_eq!(report["databases"], 2, "{report}");
    let boot = api(&router, "GET", "bootstrap", &token, json!({})).await;
    let find = |name: &str| {
        boot["databases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["name"] == name)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let people = api(
        &router,
        "GET",
        &format!("databases/{}", find("People")),
        &token,
        json!({}),
    )
    .await;
    let team = &people["columns"][1];
    assert_eq!(team["kind"], "multi_select", "{people}");
    assert_eq!(team["options"], json!(["Design", "Research", "Ops"]));
    let ada = people["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["cells"][people["columns"][0]["id"].as_str().unwrap()] == "Ada")
        .unwrap();
    assert_eq!(
        ada["cells"][team["id"].as_str().unwrap()],
        json!(["Design", "Research"])
    );

    let projects = api(
        &router,
        "GET",
        &format!("databases/{}", find("Projects")),
        &token,
        json!({}),
    )
    .await;
    let owners = &projects["columns"][1];
    assert_eq!(owners["kind"], "relation", "{projects}");
    assert_eq!(owners["target"], people["id"]);
    let title = projects["columns"][0]["id"].as_str().unwrap();
    let launch = projects["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["cells"][title] == "Launch")
        .unwrap();
    let linked = launch["cells"][owners["id"].as_str().unwrap()]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(linked, 2, "Ada and Bo are linked");
    let orphan = projects["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["cells"][title] == "Orphan")
        .unwrap();
    assert!(
        orphan["cells"]
            .get(owners["id"].as_str().unwrap())
            .is_none(),
        "a record of another table links nothing"
    );
}

#[tokio::test]
async fn imports_bear_textbundles_and_onenote_web_archives() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let root = api(
        &router,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let token = common::member(&router, &root, "owner@test.local").await;
    let boot = api(&router, "GET", "bootstrap", &token, json!({})).await;
    let target = common::team_folder(&boot)["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Bear: a TextBundle, with its picture and tags with spaces; C# stays.
    let pack = zip(&[
        ("Garden ideas.textbundle/text.md", b"# Garden ideas\n\nTomatoes #home/garden #summer plans# and C# code.\n\n![](assets/bed.png)"),
        ("Garden ideas.textbundle/info.json", b"{}"),
        ("Garden ideas.textbundle/assets/bed.png", PNG),
    ]);
    let (status, report) = import(
        &router,
        &token,
        &target,
        "bear.zip",
        zip(&[("Garden ideas.textpack", &pack)]),
    )
    .await;
    assert_eq!(status, 200, "{report}");
    let all = notes(&router, &token).await;
    let (_, garden) = &all["Garden ideas"];
    let body = text(garden);
    assert!(
        body.starts_with("Tomatoes #home/garden #summer-plans and C# code."),
        "{body}"
    );
    assert!(body.contains("![bed.png](/api/v1/attachments/"), "{body}");

    // OneNote: a single-file web page, with quoted-printable HTML and a picture.
    use base64::Engine;
    let png = base64::engine::general_purpose::STANDARD.encode(PNG);
    let mht = format!(
        "MIME-Version: 1.0\r\nContent-Type: multipart/related; boundary=\"----=_NextPart_01\"\r\n\r\n------=_NextPart_01\r\nContent-Location: file:///C:/page.htm\r\nContent-Transfer-Encoding: quoted-printable\r\nContent-Type: text/html; charset=\"utf-8\"\r\n\r\n<html><head><title>Meeting =E2=80=93 Monday</title></head><body><p>Agenda for the=\r\n week</p><ul><li>Budget</li></ul><p><img src=3D\"page_files/image001.png\"></p></body></html>\r\n------=_NextPart_01\r\nContent-Location: page_files/image001.png\r\nContent-Transfer-Encoding: base64\r\nContent-Type: image/png\r\n\r\n{png}\r\n------=_NextPart_01--\r\n"
    );
    let (status, report) = import(&router, &token, &target, "meeting.mht", mht.into_bytes()).await;
    assert_eq!(status, 200, "{report}");
    let all = notes(&router, &token).await;
    let (_, meeting) = &all["Meeting – Monday"];
    let body = text(meeting);
    assert!(body.contains("Agenda for the week"), "{body}");
    assert!(body.contains("Budget"), "{body}");
    assert!(
        body.contains("/api/v1/attachments/"),
        "the picture is kept: {body}"
    );
}
