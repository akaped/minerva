//! Background replication between the embedded server and remote servers.
//!
//! For each replicated workspace the worker first pushes local changes (read from
//! the local change log since the replica's push cursor), then pulls the server's
//! changes since its pull cursor and applies them locally. Both servers merge with
//! the same rules (see `minerva-server`'s `replication.rs`). Operation IDs derive
//! from change-log positions, so a retry after a lost response applies only once,
//! and both cursors persist in the local database, so nothing is lost on restart.
use crate::{Error, Gateway, LocalServer, Result, Session};
use http::Method;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

pub struct SyncWorker {
    local: Arc<LocalServer>,
    gateway: Arc<Gateway>,
    wake: Notify,
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl SyncWorker {
    pub fn new(local: Arc<LocalServer>, gateway: Arc<Gateway>) -> Arc<Self> {
        Arc::new(Self {
            local,
            gateway,
            wake: Notify::new(),
        })
    }
    /// Starts the next pass now, for example after the network came back.
    pub fn wake(&self) {
        self.wake.notify_one();
    }
    /// Syncs every two seconds while servers answer, backing off to five minutes
    /// while one does not.
    pub async fn run(self: Arc<Self>) {
        let mut delay = Duration::from_secs(2);
        loop {
            delay = if self.run_once().await {
                Duration::from_secs(2)
            } else {
                (delay * 2).clamp(Duration::from_secs(4), Duration::from_secs(300))
            };
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = self.wake.notified() => {}
            }
        }
    }
    /// One pass over every replica; returns whether every server was reachable.
    pub async fn run_once(&self) -> bool {
        let Ok(replicas) = self
            .local
            .call(Method::GET, "local/replicas", Value::Null)
            .await
        else {
            return false;
        };
        let mut reachable = true;
        for r in replicas.as_array().cloned().unwrap_or_default() {
            let status = match self.sync(&r).await {
                Ok(()) => json!({"state":"synced","at":now()}),
                Err(Error::Api(401, message)) => {
                    let _ = self.gateway.signed_out(text(&r, "connection_id"));
                    json!({"state":"signed_out","error":message})
                }
                Err(Error::Api(403, message)) => json!({"state":"revoked","error":message}),
                Err(e @ Error::Network(_)) => {
                    reachable = false;
                    json!({"state":"offline","error":e.to_string()})
                }
                Err(e @ Error::IdentityChanged { .. }) => {
                    json!({"state":"identity_changed","error":e.to_string()})
                }
                Err(e) => json!({"state":"error","error":e.to_string()}),
            };
            let path = format!("local/replicas/{}", text(&r, "workspace_id"));
            let _ = self
                .local
                .call(Method::PUT, &path, json!({ "status": status }))
                .await;
        }
        reachable
    }

    /// The workspaces a connection offers, marking those already replicated.
    pub async fn available(&self, connection_id: &str) -> Result<Value> {
        let remote = self.gateway.session(connection_id).await?;
        let boot = remote.bootstrap().await?;
        let replicas = self
            .local
            .call(Method::GET, "local/replicas", Value::Null)
            .await?;
        let subscribed: HashSet<&str> = replicas
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| text(r, "workspace_id"))
            .collect();
        let list: Vec<Value> = boot["workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|w| {
                json!({
                    "id": w["id"],
                    "name": w["name"],
                    "offline": subscribed.contains(text(w, "id")),
                })
            })
            .collect();
        Ok(json!(list))
    }
    /// Makes a remote workspace available offline; the next pass copies it.
    pub async fn subscribe(&self, connection_id: &str, workspace_id: &str) -> Result<()> {
        let connection = self
            .gateway
            .connection(connection_id)
            .ok_or_else(|| Error::Invalid("Unknown server connection".into()))?;
        let available = self.available(connection_id).await?;
        let workspace = available
            .as_array()
            .into_iter()
            .flatten()
            .find(|w| w["id"] == workspace_id)
            .ok_or_else(|| Error::Invalid("That workspace is not available".into()))?;
        self.local
            .call(
                Method::POST,
                "local/replicas",
                json!({
                    "workspace_id": workspace_id,
                    "connection_id": connection_id,
                    "server_id": connection.server_id,
                    "name": workspace["name"],
                }),
            )
            .await?;
        self.wake();
        Ok(())
    }
    /// Removes the local copy; refused while changes are unsent unless `discard`.
    pub async fn unsubscribe(&self, workspace_id: &str, discard: bool) -> Result<()> {
        self.local
            .call(
                Method::DELETE,
                &format!("local/replicas/{workspace_id}"),
                json!({ "discard": discard }),
            )
            .await
            .map(|_| ())
    }

    async fn sync(&self, r: &Value) -> Result<()> {
        let connection = self
            .gateway
            .connection(text(r, "connection_id"))
            .ok_or_else(|| Error::Invalid("This workspace's server was removed".into()))?;
        if connection.server_id != text(r, "server_id") {
            return Err(Error::IdentityChanged {
                expected: text(r, "server_id").into(),
                found: connection.server_id,
            });
        }
        let remote = self.gateway.session(&connection.id).await?;
        self.push(&remote, r).await?;
        self.pull(&remote, r).await
    }

    async fn push(&self, remote: &Session, r: &Value) -> Result<()> {
        let ws = text(r, "workspace_id");
        let watermark = text(r, "watermark");
        let mut cursor = r["push_cursor"].as_i64().unwrap_or(0);
        let mut next_watermark = None;
        loop {
            let outbox = self
                .local
                .call(
                    Method::GET,
                    &format!("sync/changes?workspace={ws}&since={cursor}&local_only=1&limit=100"),
                    Value::Null,
                )
                .await?;
            let changes = outbox["changes"].as_array().cloned().unwrap_or_default();
            if changes.is_empty() {
                break;
            }
            let node = text(&outbox, "server_id");
            // Every field stamped before this read is in this or a later batch.
            next_watermark.get_or_insert_with(|| outbox["hlc"].clone());
            let ops: Vec<Value> = changes
                .iter()
                .map(|e| {
                    let mut op = e.clone();
                    let object = op.as_object_mut().unwrap();
                    object.remove("role");
                    let seq = object.remove("seq").unwrap_or_default();
                    // Fields edited here since the last push; a conflict on any
                    // other field is just an older value being replaced.
                    let changed: Vec<&String> = e["clocks"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter(|(_, clock)| {
                            let clock = clock.as_str().unwrap_or_default();
                            clock.rsplit('.').next() == Some(node) && clock > watermark
                        })
                        .map(|(field, _)| field)
                        .collect();
                    op["changed"] = json!(changed);
                    op["op_id"] = json!(format!("{node}:{seq}"));
                    op
                })
                .collect();
            let pushed = remote
                .request(
                    Method::POST,
                    "sync/push",
                    Some(json!({ "workspace_id": ws, "ops": ops })),
                )
                .await?;
            let mut conflicts = vec![];
            let mut restore = vec![];
            for (op, result) in ops
                .iter()
                .zip(pushed["results"].as_array().into_iter().flatten())
            {
                let changed: HashSet<&str> = op["changed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let lost: Vec<&str> = result["lost"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|f| changed.contains(f))
                    .collect();
                match text(result, "status") {
                    "applied" => {}
                    "merged" if lost.is_empty() => {}
                    status => {
                        if result["kept"] == true && result["entity"]["deleted"] != true {
                            let mut entity = result["entity"].clone();
                            entity["op_id"] = json!(format!("kept:{}", text(op, "op_id")));
                            restore.push(entity);
                        }
                        conflicts.push(json!({
                            "workspace_id": ws,
                            "kind": op["kind"],
                            "entity_id": op["id"],
                            "status": status,
                            "kept": result["kept"],
                            "lost": lost,
                            "copy_id": result["copy_id"],
                            "error": result["error"],
                        }));
                    }
                }
                if op["kind"] == "note" && op["deleted"] != true {
                    self.copy_attachments(remote, text(op, "id"), false).await?;
                }
            }
            if !restore.is_empty() {
                self.local
                    .call(
                        Method::POST,
                        "sync/push",
                        json!({ "workspace_id": ws, "upstream": true, "ops": restore }),
                    )
                    .await?;
            }
            if !conflicts.is_empty() {
                self.local
                    .call(
                        Method::POST,
                        "local/conflicts",
                        json!({ "items": conflicts }),
                    )
                    .await?;
            }
            cursor = outbox["cursor"].as_i64().unwrap_or(cursor);
            self.local
                .call(
                    Method::PUT,
                    &format!("local/replicas/{ws}"),
                    json!({ "push_cursor": cursor }),
                )
                .await?;
            if outbox["more"] != true {
                break;
            }
        }
        if let Some(watermark) = next_watermark {
            self.local
                .call(
                    Method::PUT,
                    &format!("local/replicas/{ws}"),
                    json!({ "watermark": watermark }),
                )
                .await?;
        }
        Ok(())
    }

    async fn pull(&self, remote: &Session, r: &Value) -> Result<()> {
        let ws = text(r, "workspace_id");
        let mut cursor = r["pull_cursor"].as_i64().unwrap_or(0);
        let mut access = text(r, "access").to_string();
        loop {
            let pulled = remote
                .request(
                    Method::GET,
                    &format!("sync/changes?workspace={ws}&since={cursor}&limit=200"),
                    None,
                )
                .await?;
            if pulled["revoked"] == true {
                self.local
                    .call(
                        Method::PUT,
                        &format!("local/replicas/{ws}"),
                        json!({"revoked":true}),
                    )
                    .await?;
                return Err(Error::Api(
                    403,
                    "Your access to this workspace was removed. Its local copy is read-only."
                        .into(),
                ));
            }
            if r["revoked"] == true {
                self.local
                    .call(
                        Method::PUT,
                        &format!("local/replicas/{ws}"),
                        json!({"revoked":false}),
                    )
                    .await?;
            }
            let digest = text(&pulled, "access").to_string();
            // Changed access can hide or reveal entities without changing them:
            // read everything again; entities now hidden arrive as tombstones.
            if cursor > 0 && !access.is_empty() && digest != access {
                cursor = 0;
                access = digest;
                continue;
            }
            let changes = pulled["changes"].as_array().cloned().unwrap_or_default();
            if changes.is_empty() && digest == access {
                break;
            }
            access = digest.clone();
            let server = text(&pulled, "server_id");
            let tag: String = digest.chars().take(8).collect();
            let ops: Vec<Value> = changes
                .iter()
                .map(|e| {
                    let mut op = e.clone();
                    let seq = op
                        .as_object_mut()
                        .unwrap()
                        .remove("seq")
                        .unwrap_or_default();
                    op["op_id"] = json!(format!("{server}:{tag}:{seq}"));
                    op
                })
                .collect();
            let applied = self
                .local
                .call(
                    Method::POST,
                    "sync/push",
                    json!({
                        "workspace_id": ws,
                        "upstream": true,
                        "ops": ops,
                        "cursor": pulled["cursor"],
                        "access": digest,
                    }),
                )
                .await?;
            let failed: Vec<Value> = ops
                .iter()
                .zip(applied["results"].as_array().into_iter().flatten())
                .filter(|(_, result)| {
                    !matches!(text(result, "status"), "applied" | "merged" | "conflict")
                })
                .map(|(op, result)| {
                    json!({
                        "workspace_id": ws,
                        "kind": op["kind"],
                        "entity_id": op["id"],
                        "status": result["status"],
                        "error": result["error"],
                        "incoming": true,
                    })
                })
                .collect();
            if !failed.is_empty() {
                self.local
                    .call(Method::POST, "local/conflicts", json!({ "items": failed }))
                    .await?;
            }
            for op in &ops {
                if op["kind"] == "note" && op["deleted"] != true {
                    self.copy_attachments(remote, text(op, "id"), true).await?;
                }
            }
            cursor = pulled["cursor"].as_i64().unwrap_or(cursor);
            if pulled["more"] != true {
                break;
            }
        }
        Ok(())
    }

    /// Copies the note's attachments that one side lacks. Only network failures
    /// stop the pass; a refused file is retried when the note changes again.
    async fn copy_attachments(&self, remote: &Session, note: &str, to_local: bool) -> Result<()> {
        let path = format!("notes/{note}/attachments");
        let list = |v: Value| -> Vec<(String, String)> {
            v["attachments"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|a| (text(a, "id").to_string(), text(a, "filename").to_string()))
                .collect()
        };
        let theirs = match remote.request(Method::GET, &path, None).await {
            Ok(v) => list(v),
            Err(e @ Error::Network(_)) => return Err(e),
            Err(_) => return Ok(()),
        };
        let Ok(mine) = self
            .local
            .call(Method::GET, &path, Value::Null)
            .await
            .map(list)
        else {
            return Ok(());
        };
        let (source, target) = if to_local {
            (&theirs, &mine)
        } else {
            (&mine, &theirs)
        };
        let present: HashSet<&str> = target.iter().map(|(id, _)| id.as_str()).collect();
        for (id, filename) in source
            .iter()
            .filter(|(id, _)| !present.contains(id.as_str()))
        {
            let copied = if to_local {
                match remote.attachment(id).await {
                    Ok(bytes) => self.local.put_attachment(note, id, filename, bytes).await,
                    Err(e) => Err(e),
                }
            } else {
                match self.local.attachment(id).await {
                    Ok(bytes) => remote.put_attachment(note, id, filename, bytes).await,
                    Err(e) => Err(e),
                }
            };
            if let Err(e @ Error::Network(_)) = copied {
                return Err(e);
            }
        }
        Ok(())
    }
}
