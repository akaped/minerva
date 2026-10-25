//! Commands from the launcher page. Only the launcher origin may issue them;
//! server pages are limited to returning to the launcher.
use crate::origin;
use minerva_client::{Error, Gateway, SyncWorker};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct Message {
    #[serde(default)]
    pub id: u64,
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
}

pub enum Effect {
    None,
    Navigate(String),
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, Error> {
    args[key]
        .as_str()
        .ok_or_else(|| Error::Invalid(format!("Missing {key}")))
}
fn open(id: &str) -> Effect {
    Effect::Navigate(origin::page_url(&origin::connection_host(id), "/"))
}

pub async fn dispatch(
    gateway: &Gateway,
    worker: &SyncWorker,
    message: &Message,
) -> (Value, Effect) {
    let args = &message.args;
    let result: Result<(Value, Effect), Error> = async {
        Ok(match message.cmd.as_str() {
            "list" => {
                let list: Vec<Value> = gateway
                    .connections()
                    .into_iter()
                    .map(|c| {
                        json!({
                            "id": c.id, "label": c.label, "url": c.url, "account": c.account,
                            "signed_in": gateway.signed_in(&c.id),
                        })
                    })
                    .collect();
                (json!(list), Effect::None)
            }
            "add" => {
                let connection = gateway
                    .add(
                        text(args, "url")?,
                        args["label"].as_str().unwrap_or(""),
                        args["allow_insecure_http"].as_bool().unwrap_or(false),
                    )
                    .await?;
                (
                    json!({ "id": connection.id, "label": connection.label }),
                    Effect::None,
                )
            }
            "open_local" => (
                Value::Null,
                Effect::Navigate(origin::page_url(origin::LOCAL, "/")),
            ),
            "workspaces" => (worker.available(text(args, "id")?).await?, Effect::None),
            "offline" => {
                let workspace = text(args, "workspace_id")?;
                if args["enabled"].as_bool() == Some(true) {
                    worker.subscribe(text(args, "id")?, workspace).await?;
                } else {
                    worker
                        .unsubscribe(workspace, args["discard"].as_bool() == Some(true))
                        .await?;
                }
                (Value::Null, Effect::None)
            }
            "open" => {
                let id = text(args, "id")?;
                gateway.verify(id).await?;
                (Value::Null, open(id))
            }
            "trust" => {
                let id = text(args, "id")?;
                gateway.trust_current_identity(id).await?;
                (Value::Null, open(id))
            }
            "rename" => {
                gateway.rename(text(args, "id")?, text(args, "label")?)?;
                (Value::Null, Effect::None)
            }
            "sign_out" => {
                gateway.sign_out(text(args, "id")?).await?;
                (Value::Null, Effect::None)
            }
            "remove" => {
                gateway.remove(text(args, "id")?).await?;
                (Value::Null, Effect::None)
            }
            other => return Err(Error::Invalid(format!("Unknown command {other}"))),
        })
    }
    .await;
    match result {
        Ok((value, effect)) => (json!({ "ok": true, "value": value }), effect),
        Err(e) => (
            json!({
                "ok": false,
                "error": e.to_string(),
                "identity_changed": matches!(e, Error::IdentityChanged { .. }),
                "signed_out": matches!(e, Error::Api(401, _)),
                "unsent": matches!(e, Error::Api(409, _)),
            }),
            Effect::None,
        ),
    }
}
