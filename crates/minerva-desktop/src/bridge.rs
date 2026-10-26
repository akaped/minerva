//! Lets this computer's page (the local server's interface) show the servers
//! the app knows, with their workspaces, in its workspace menu, and add servers
//! or offline copies from there. Only that page may call it: a server's page
//! must never learn which other servers or workspaces you use.
use crate::origin;
use http::{Method, Request, Response, StatusCode};
use minerva_client::{Error, Gateway, SyncWorker};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

pub const PREFIX: &str = "/desktop/";

fn reply(status: StatusCode, body: Value) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(body.to_string().into_bytes())
        .unwrap()
}
fn failure(e: Error) -> Response<Vec<u8>> {
    let status = match e {
        Error::Api(401, _) => StatusCode::UNAUTHORIZED,
        Error::Network(_) => StatusCode::BAD_GATEWAY,
        _ => StatusCode::BAD_REQUEST,
    };
    reply(status, json!({ "error": e.to_string() }))
}
/// Requests from this computer's page only.
fn from_local_page(request: &Request<Vec<u8>>) -> bool {
    let local = origin::origin(origin::LOCAL);
    let header = |name| request.headers().get(name).and_then(|v| v.to_str().ok());
    match header("origin") {
        Some(o) => o == local,
        None => header("referer").is_some_and(|r| r.starts_with(&format!("{local}/"))),
    }
}

pub async fn handle(
    gateway: Arc<Gateway>,
    worker: Arc<SyncWorker>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if !from_local_page(&request) {
        return reply(StatusCode::FORBIDDEN, json!({"error":"Not available here"}));
    }
    let write = request.method() != Method::GET;
    if write
        && request
            .headers()
            .get("x-minerva-client")
            .and_then(|v| v.to_str().ok())
            != Some("1")
    {
        return reply(
            StatusCode::BAD_REQUEST,
            json!({"error":"X-Minerva-Client: 1 is required"}),
        );
    }
    let body: Value = serde_json::from_slice(request.body()).unwrap_or(Value::Null);
    let text = |key: &str| body[key].as_str().unwrap_or("").to_string();
    let path = request.uri().path().trim_start_matches(PREFIX).to_string();
    match (request.method().clone(), path.as_str()) {
        (Method::GET, "servers") => reply(StatusCode::OK, json!(servers(&gateway, &worker).await)),
        (Method::POST, "servers") => match gateway
            .add(
                &text("url"),
                &text("label"),
                body["allow_insecure_http"].as_bool().unwrap_or(false),
            )
            .await
        {
            Ok(c) => reply(StatusCode::OK, json!({"id": c.id, "label": c.label})),
            Err(e) => failure(e),
        },
        (Method::POST, "offline") => {
            let result = if body["enabled"].as_bool() == Some(true) {
                worker.subscribe(&text("id"), &text("workspace_id")).await
            } else {
                worker
                    .unsubscribe(
                        &text("workspace_id"),
                        body["discard"].as_bool() == Some(true),
                    )
                    .await
            };
            match result {
                Ok(()) => reply(StatusCode::OK, json!({"ok": true})),
                Err(e) => failure(e),
            }
        }
        _ => reply(StatusCode::NOT_FOUND, json!({"error":"Not found"})),
    }
}

/// Every server with its workspaces, asked in parallel; a server that does not
/// answer within a few seconds is shown as unreachable.
async fn servers(gateway: &Arc<Gateway>, worker: &Arc<SyncWorker>) -> Vec<Value> {
    let tasks: Vec<_> = gateway
        .connections()
        .into_iter()
        .map(|c| {
            let (gateway, worker) = (gateway.clone(), worker.clone());
            tokio::spawn(async move {
                let open = origin::page_url(&origin::connection_host(&c.id), "/");
                let mut entry = json!({
                    "id": c.id, "label": c.label, "url": c.url, "account": c.account,
                    "signed_in": gateway.signed_in(&c.id), "open": open,
                });
                match tokio::time::timeout(Duration::from_secs(5), worker.available(&c.id)).await {
                    Ok(Ok(list)) => entry["workspaces"] = list,
                    Ok(Err(Error::Api(401, _))) => entry["state"] = json!("signed_out"),
                    Ok(Err(e)) => {
                        entry["state"] = json!("unreachable");
                        entry["error"] = json!(e.to_string());
                    }
                    Err(_) => entry["state"] = json!("unreachable"),
                }
                entry
            })
        })
        .collect();
    let mut out = vec![];
    for task in tasks {
        if let Ok(entry) = task.await {
            out.push(entry);
        }
    }
    out
}
