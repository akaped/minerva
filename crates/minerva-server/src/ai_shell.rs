//! Local desktop tool. The network server never enables this runtime gate.
use super::*;
use tokio::sync::oneshot;

pub(super) struct Pending {
    pub user: String,
    pub chat: String,
    pub command: String,
    pub directory: String,
    pub reply: oneshot::Sender<bool>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Settings {
    pub directory: String,
    #[serde(default)]
    pub unattended: bool,
}
fn settings(s: &Store) -> Result<Settings, ApiError> {
    let raw: String =
        s.db.query_row("SELECT value FROM ai_shell_settings WHERE id=1", [], |r| {
            r.get(0)
        })
        .map_err(|_| internal())?;
    serde_json::from_str(&raw).map_err(|_| internal())
}
pub(super) fn route(
    s: &mut Store,
    u: &User,
    method: &Method,
    parts: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    if !s.ai.desktop {
        return Err(forbidden());
    }
    admin(u)?;
    match (method.as_str(), parts) {
        ("GET", ["shell", "settings"]) => Ok(json!(settings(s)?)),
        ("PUT", ["shell", "settings"]) => {
            let config: Settings =
                serde_json::from_value(v.clone()).map_err(|_| bad("Invalid shell settings"))?;
            if !config.directory.is_empty() {
                let path = Path::new(&config.directory);
                if !path.is_absolute() || !path.is_dir() {
                    return Err(bad("Choose an existing absolute working folder"));
                }
            }
            if config.unattended && config.directory.is_empty() {
                return Err(bad("Choose an existing absolute working folder"));
            }
            s.db.execute(
                "UPDATE ai_shell_settings SET value=?1 WHERE id=1",
                [serde_json::to_string(&config).map_err(|_| internal())?],
            )
            .map_err(|_| internal())?;
            // Changing policy invalidates pending approvals tied to the old folder.
            s.ai.shell_pending.clear();
            Ok(json!(config))
        }
        ("GET", ["shell", "pending", chat]) => {
            load_chat(s, u, chat)?;
            Ok(json!(s
                .ai
                .shell_pending
                .iter()
                .filter(|(_, p)| p.user == u.id && p.chat == *chat)
                .map(|(id, p)| json!({"id":id,"command":p.command,"directory":p.directory}))
                .collect::<Vec<_>>()))
        }
        ("POST", ["shell", "approval", key]) => {
            let pending = s.ai.shell_pending.get(*key).ok_or_else(missing)?;
            if pending.user != u.id {
                return Err(forbidden());
            }
            let pending = s.ai.shell_pending.remove(*key).ok_or_else(missing)?;
            let _ = pending.reply.send(v["approved"] == true);
            Ok(json!({"ok":true}))
        }
        ("POST", ["shell", "cancel", chat]) => {
            load_chat(s, u, chat)?;
            s.ai.shell_pending
                .retain(|_, p| p.user != u.id || p.chat != *chat);
            s.ai.shell_cancelled.insert(chat.to_string());
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}

pub(super) fn definition() -> Value {
    json!({"type":"function", "name":"minerva_shell", "description":"Run a command on the user's desktop computer in their configured working folder. Requires user approval unless they enabled unattended execution. Output is sent to the model. Do not use for background services.", "strict":true,
        "parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"],"additionalProperties":false}})
}

pub(super) async fn run(
    store: &Shared,
    headers: &HeaderMap,
    user: &User,
    chat: &Chat,
    provider_id: &str,
    command: &str,
) -> Result<String, ApiError> {
    if command.trim().is_empty() || command.len() > 8000 || command.contains('\0') {
        return Err(bad("Invalid shell command"));
    }
    let (config, pending) = {
        let mut s = store.lock().map_err(|_| internal())?;
        authenticate(&s.data, headers)?;
        admin(user)?;
        chat_live(&s, user, chat)?;
        if !s.ai.desktop
            || s.ai.shell_cancelled.contains(&chat.id)
            || !provider(&s, provider_id)?
                .0
                .server_tools
                .iter()
                .any(|t| t == "minerva:shell")
        {
            return Err(forbidden());
        }
        let config = settings(&s)?;
        if config.directory.is_empty() {
            return Err(bad("Choose an existing absolute working folder"));
        }
        let pending = if config.unattended {
            None
        } else {
            let (tx, rx) = oneshot::channel();
            let key = id();
            s.ai.shell_pending.insert(
                key.clone(),
                Pending {
                    user: user.id.clone(),
                    chat: chat.id.clone(),
                    command: command.into(),
                    directory: config.directory.clone(),
                    reply: tx,
                },
            );
            Some((key, rx))
        };
        (config, pending)
    };
    if let Some((key, rx)) = pending {
        let approved = tokio::time::timeout(Duration::from_secs(120), rx)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(false);
        let mut s = store.lock().map_err(|_| internal())?;
        s.ai.shell_pending.remove(&key);
        if !approved {
            return Ok("The user denied the command or approval expired. Do not retry it.".into());
        }
    }
    {
        let s = store.lock().map_err(|_| internal())?;
        authenticate(&s.data, headers)?;
        chat_live(&s, user, chat)?;
        if s.ai.shell_cancelled.contains(&chat.id)
            || settings(&s)?.directory != config.directory
            || !provider(&s, provider_id)?
                .0
                .server_tools
                .iter()
                .any(|t| t == "minerva:shell")
        {
            return Err(forbidden());
        }
    }
    let output = execute(command, &config.directory, store, &chat.id).await?;
    if store
        .lock()
        .map_err(|_| internal())?
        .ai
        .shell_cancelled
        .contains(&chat.id)
    {
        return Err(forbidden());
    }
    Ok(output)
}

async fn execute(
    command: &str,
    directory: &str,
    store: &Shared,
    chat: &str,
) -> Result<String, ApiError> {
    use tokio::io::AsyncReadExt;
    #[cfg(windows)]
    let mut process = {
        let mut c = tokio::process::Command::new("cmd.exe");
        c.args(["/D", "/S", "/C", command]);
        c
    };
    #[cfg(not(windows))]
    let mut process = {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.args(["-c", command]);
        c
    };
    process
        .current_dir(directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        process.process_group(0);
    }
    let mut child = process
        .spawn()
        .map_err(|_| bad("Shell command could not start"))?;
    let pid = child.id().unwrap_or(0);
    let mut stdout = child.stdout.take().ok_or_else(internal)?.take(32_768);
    let mut stderr = child.stderr.take().ok_or_else(internal)?.take(32_768);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let cancelled = async {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let stop = store
                .lock()
                .map(|s| s.ai.shell_cancelled.contains(chat))
                .unwrap_or(true);
            if stop {
                break;
            }
        }
    };
    let work = async {
        let (status, a, b) = tokio::join!(
            child.wait(),
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err)
        );
        a?;
        b?;
        status
    };
    let result = tokio::select! {
        result=tokio::time::timeout(Duration::from_secs(30),work) => result.ok().and_then(Result::ok),
        _=cancelled => None,
    };
    // Best-effort cleanup of descendants in the process group. This is not a sandbox.
    if pid > 0 {
        #[cfg(unix)]
        {
            let _ = tokio::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{pid}")])
                .output()
                .await;
        }
        #[cfg(windows)]
        {
            let _ = tokio::process::Command::new("taskkill.exe")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output()
                .await;
        }
    }
    let status = match result {
        Some(status) => status
            .code()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "signal".into()),
        _ => {
            let _ = child.kill().await;
            "timeout or failure".into()
        }
    };
    Ok(format!(
        "Exit: {status}\nstdout (up to 32 KiB):\n{}\nstderr (up to 32 KiB):\n{}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    ))
}
