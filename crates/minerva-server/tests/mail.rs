//! Email, against a fake SMTP server: invitations, password resets, access
//! grants, assignments in notes, and reminders.
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::Engine;
use http_body_util::BodyExt;
use minerva_server::{app, Store};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;
mod common;

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
async fn login(r: &Router, email: &str, password: &str) -> String {
    let (_, v) = call(
        r,
        "POST",
        "login",
        "",
        json!({"email":email,"password":password}),
    )
    .await;
    v["token"].as_str().unwrap_or("").into()
}

/// A minimal SMTP server that keeps each message's recipient and decoded text.
async fn smtp() -> (u16, Arc<Mutex<Vec<(String, String)>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let inbox = Arc::new(Mutex::new(vec![]));
    let kept = inbox.clone();
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let inbox = kept.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"220 fake\r\n").await.unwrap();
                let (mut to, mut data, mut in_data) = (String::new(), String::new(), false);
                while let Ok(Some(line)) = lines.next_line().await {
                    if in_data {
                        if line == "." {
                            in_data = false;
                            inbox.lock().unwrap().push((to.clone(), decode(&data)));
                            data.clear();
                            write.write_all(b"250 queued\r\n").await.unwrap();
                        } else {
                            data += &line;
                            data += "\n";
                        }
                        continue;
                    }
                    let upper = line.to_uppercase();
                    let reply: &[u8] = if upper.starts_with("EHLO") {
                        b"250-fake\r\n250 8BITMIME\r\n"
                    } else if upper.starts_with("RCPT TO:") {
                        to = line[8..]
                            .trim_matches(|c| c == '<' || c == '>' || c == ' ')
                            .into();
                        b"250 ok\r\n"
                    } else if upper == "DATA" {
                        in_data = true;
                        b"354 go\r\n"
                    } else if upper == "QUIT" {
                        write.write_all(b"221 bye\r\n").await.unwrap();
                        break;
                    } else {
                        b"250 ok\r\n"
                    };
                    write.write_all(reply).await.unwrap();
                }
            });
        }
    });
    (port, inbox)
}
/// The message's text parts, undoing quoted-printable and base64.
fn decode(raw: &str) -> String {
    let mut out = raw.replace("=\n", "").replace("=3D", "=");
    for block in raw.split("\n\n") {
        let joined: String = block.lines().collect();
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(joined.trim()) {
            out += &String::from_utf8_lossy(&bytes);
        }
    }
    out
}
async fn wait_for(
    inbox: &Arc<Mutex<Vec<(String, String)>>>,
    count: usize,
) -> Vec<(String, String)> {
    for _ in 0..100 {
        if inbox.lock().unwrap().len() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    inbox.lock().unwrap().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_emails_people() {
    let (port, inbox) = smtp().await;
    std::env::set_var("MINERVA_SMTP_HOST", "127.0.0.1");
    std::env::set_var("MINERVA_SMTP_PORT", port.to_string());
    std::env::set_var("MINERVA_SMTP_SECURITY", "none");
    std::env::set_var("MINERVA_MAIL_FROM", "Minerva <minerva@test.local>");
    std::env::set_var("MINERVA_PUBLIC_URL", "https://notes.test");
    let r = app(Store::open(":memory:", "admin@test.local", "test-password-1234").unwrap());
    let root = login(&r, "admin@test.local", "test-password-1234").await;
    let admin = common::member(&r, &root, "owner@test.local").await;
    // The owner's own invitation goes first; the checks below start after it.
    assert_eq!(wait_for(&inbox, 1).await[0].0, "owner@test.local");
    inbox.lock().unwrap().clear();
    let (_, health) = call(&r, "GET", "health", "", json!({})).await;
    assert_eq!(health["mail"], true);

    // A new member is invited with a link to choose a password.
    let (_, ada) = call(
        &r,
        "POST",
        "users",
        &root,
        json!({"name":"Ada","email":"ada@test.local","password":"temporary-pass-1234"}),
    )
    .await;
    assert_eq!(ada["invited"], true);
    let mail = wait_for(&inbox, 1).await;
    assert_eq!(mail[0].0, "ada@test.local");
    let link = mail[0]
        .1
        .split("https://notes.test/?reset=")
        .nth(1)
        .expect("reset link");
    let token: String = link
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let (status, _) = call(
        &r,
        "POST",
        "password-reset/complete",
        "",
        json!({"token":token,"password":"ada-chosen-pass-1234"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &r,
        "POST",
        "password-reset/complete",
        "",
        json!({"token":token,"password":"another-pass-12345"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a link works once");
    let ada_token = login(&r, "ada@test.local", "ada-chosen-pass-1234").await;
    assert!(!ada_token.is_empty());

    // Forgotten passwords: the same answer for unknown addresses, and no mail.
    let (status, _) = call(
        &r,
        "POST",
        "password-reset",
        "",
        json!({"email":"nobody@test.local"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    call(
        &r,
        "POST",
        "password-reset",
        "",
        json!({"email":"ADA@test.local"}),
    )
    .await;
    let mail = wait_for(&inbox, 2).await;
    assert_eq!(mail.len(), 2);
    assert!(mail[1].1.contains("Reset your Minerva password") || mail[1].1.contains("?reset="));

    // Sharing a workspace tells the person.
    call(&r, "POST", "workspaces", &admin, json!({"name":"Team"})).await;
    let (_, boot) = call(&r, "GET", "bootstrap", &admin, json!({})).await;
    let team = boot["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["personal"] != true)
        .unwrap();
    let wid = team["id"].as_str().unwrap();
    let uid = ada["id"].as_str().unwrap();
    call(
        &r,
        "PUT",
        "workspace-access",
        &admin,
        json!({"workspace_id":wid,"user_id":uid,"role":"editor"}),
    )
    .await;
    let mail = wait_for(&inbox, 3).await;
    assert_eq!(mail[2].0, "ada@test.local");
    // Sharing it again with the same role does not.
    call(
        &r,
        "PUT",
        "workspace-access",
        &admin,
        json!({"workspace_id":wid,"user_id":uid,"role":"editor"}),
    )
    .await;

    // Assigning someone in a note mails them the block, once.
    let folder = boot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["workspace_id"] == wid)
        .unwrap();
    let (_, note) = call(
        &r,
        "POST",
        "notes",
        &admin,
        json!({"folder_id":folder["id"],"title":"Plan"}),
    )
    .await;
    let nid = note["id"].as_str().unwrap();
    let mut note = call(&r, "GET", &format!("notes/{nid}"), &admin, json!({}))
        .await
        .1;
    note["blocks"] = json!([{"id":"b1","markdown":"Write the report @ada@test.local","x":0,"y":0}]);
    let (status, saved) = call(&r, "PUT", &format!("notes/{nid}"), &admin, note.clone()).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let mail = wait_for(&inbox, 4).await;
    assert_eq!(mail.len(), 4);
    assert_eq!(mail[3].0, "ada@test.local");
    assert!(mail[3]
        .1
        .contains(&format!("https://notes.test/?note={nid}")));
    let mut again = saved.clone();
    again["blocks"][0]["markdown"] = json!("Write the full report @ada@test.local");
    call(&r, "PUT", &format!("notes/{nid}"), &admin, again).await;

    // A reminder sends the block when it is due.
    let mut with_time = call(&r, "GET", &format!("notes/{nid}"), &ada_token, json!({}))
        .await
        .1;
    with_time["blocks"][0]["markdown"] = json!("Send the invoices ⏰2026-01-01 09:00");
    call(&r, "PUT", &format!("notes/{nid}"), &ada_token, with_time).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (status, v) = call(
        &r,
        "POST",
        "reminders",
        &ada_token,
        json!({"note_id":nid,"block_id":"b1","at":now}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let mail = wait_for(&inbox, 5).await;
    assert_eq!(
        mail.len(),
        5,
        "{:?}",
        mail.iter().map(|m| &m.0).collect::<Vec<_>>()
    );
    assert_eq!(mail[4].0, "ada@test.local");
    assert!(mail[4].1.contains("Send the invoices"));
    let (_, list) = call(&r, "GET", "reminders", &ada_token, json!({})).await;
    assert!(list["reminders"].as_array().unwrap().is_empty());
}
