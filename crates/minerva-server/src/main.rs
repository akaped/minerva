use minerva_server::{app, Store};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Container tools pass unset optional settings as empty strings; treat them as
    // unset. Done before the runtime starts any threads.
    for (key, value) in std::env::vars() {
        if key.starts_with("MINERVA_") && value.trim().is_empty() {
            std::env::remove_var(key);
        }
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("MINERVA_DATABASE").unwrap_or_else(|_| "data/minerva.db".into());
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let email =
        std::env::var("MINERVA_ADMIN_EMAIL").unwrap_or_else(|_| "admin@minerva.local".into());
    let password = std::env::var("MINERVA_ADMIN_PASSWORD").unwrap_or_default();
    if !std::path::Path::new(&path).exists() && password.len() < 12 {
        return Err("First run requires MINERVA_ADMIN_PASSWORD (at least 12 characters). Optionally set MINERVA_ADMIN_EMAIL.".into());
    }
    let state = Store::open(&path, &email, &password)?;
    let address = std::env::var("MINERVA_BIND").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("Minerva listening on http://{address}");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

/// Ctrl+C, or SIGTERM from `docker stop` and service managers.
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
