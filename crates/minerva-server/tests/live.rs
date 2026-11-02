//! Change signals over `/api/v1/live`: sent to readers of a watched note when its
//! text or the people in it change, never to others, and only for this server's
//! own pages.
use futures_util::{SinkExt, StreamExt};
use minerva_server::{app, Store};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
mod common;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn http(base: &str, method: &str, path: &str, token: &str, body: Value) -> Value {
    let client = reqwest::Client::new();
    let request = client
        .request(method.parse().unwrap(), format!("{base}/api/v1/{path}"))
        .header("x-minerva-client", "1")
        .bearer_auth(token)
        .json(&body);
    request
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap_or(Value::Null)
}
async fn connect(base: &str, token: &str, origin: &str) -> Result<Socket, u16> {
    let mut request = format!("{}/api/v1/live", base.replace("http", "ws"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => Ok(socket),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            Err(response.status().as_u16())
        }
        Err(e) => panic!("{e}"),
    }
}
/// The next signal within a second, if any.
async fn signal(socket: &mut Socket) -> Option<Value> {
    loop {
        match tokio::time::timeout(Duration::from_millis(1000), socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => return serde_json::from_str(&text).ok(),
            Ok(Some(Ok(_))) => continue,
            _ => return None,
        }
    }
}

#[tokio::test]
async fn readers_hear_when_a_watched_note_changes() {
    let router = app(Store::open(":memory:", "admin@test.local", common::PASSWORD).unwrap());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    let root = http(
        &base,
        "POST",
        "login",
        "",
        json!({"email":"admin@test.local","password":common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let owner = common_member(&base, &root, "owner@test.local").await;
    let stranger = common_member(&base, &root, "stranger@test.local").await;
    let boot = http(&base, "GET", "bootstrap", &owner, json!({})).await;
    let folder = boot["folders"][0]["id"].as_str().unwrap().to_string();
    let note = http(
        &base,
        "POST",
        "notes",
        &owner,
        json!({"folder_id":folder,"title":"Shared plan"}),
    )
    .await;
    let id = note["id"].as_str().unwrap().to_string();

    // Only this server's pages may connect.
    assert_eq!(
        connect(&base, &owner, "https://evil.example").await.err(),
        Some(403)
    );
    assert_eq!(connect(&base, "nobody", &base).await.err(), Some(401));

    let mut reader = connect(&base, &owner, &base).await.unwrap();
    let mut other = connect(&base, &stranger, &base).await.unwrap();
    for socket in [&mut reader, &mut other] {
        socket
            .send(Message::Text(json!({"watch":[id]}).to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    // A text change.
    let mut save = http(&base, "GET", &format!("notes/{id}"), &owner, json!({})).await;
    save["blocks"] = json!([{"id":"b1","markdown":"First step"}]);
    http(&base, "PUT", &format!("notes/{id}"), &owner, save).await;
    assert_eq!(signal(&mut reader).await, Some(json!({"note": id})));
    assert_eq!(
        signal(&mut other).await,
        None,
        "never to someone who cannot read it"
    );

    // Someone opening the note, and moving to a block: a signal each time; the
    // same presence again is quiet.
    let sync = |block: Option<&str>| json!({"update":"","client_id":"tab-1","active_block":block,"editing":false});
    http(
        &base,
        "POST",
        &format!("notes/{id}/sync"),
        &owner,
        sync(None),
    )
    .await;
    assert!(signal(&mut reader).await.is_some());
    http(
        &base,
        "POST",
        &format!("notes/{id}/sync"),
        &owner,
        sync(Some("b1")),
    )
    .await;
    assert!(signal(&mut reader).await.is_some());
    http(
        &base,
        "POST",
        &format!("notes/{id}/sync"),
        &owner,
        sync(Some("b1")),
    )
    .await;
    assert_eq!(signal(&mut reader).await, None);

    // A new comment is a change too.
    http(
        &base,
        "POST",
        &format!("notes/{id}/comments"),
        &owner,
        json!({"text":"Looks good"}),
    )
    .await;
    assert_eq!(signal(&mut reader).await, Some(json!({"note": id})));
}

/// `common::member`, over HTTP: an account with its own workspace.
async fn common_member(base: &str, root: &str, email: &str) -> String {
    http(
        base,
        "POST",
        "users",
        root,
        json!({"name":email,"email":email,"password":common::PASSWORD}),
    )
    .await;
    http(
        base,
        "POST",
        "login",
        "",
        json!({"email":email,"password":common::PASSWORD}),
    )
    .await["token"]
        .as_str()
        .unwrap()
        .to_string()
}
