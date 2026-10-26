//! Minerva desktop client for Windows, macOS and Linux.
//!
//! A native window hosts the system webview (WebView2 on Windows, WKWebView on
//! macOS, WebKitGTK on Linux). The app opens its own embedded server first: personal notes and
//! offline copies of remote workspaces, kept in sync in the background. Each
//! registered server's own web interface is also available online through the
//! `minerva-client` gateway, while sign-in tokens stay in the OS credential store.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod bridge;
mod chrome;
mod downloads;
mod ipc;
#[cfg(target_os = "linux")]
mod media;
mod origin;
#[cfg(target_os = "linux")]
mod print;

use chrome::Command;
use http::{Request, Response, StatusCode};
use minerva_client::{
    Gateway, KeyringStore, LocalServer, MemoryStore, Registry, SyncWorker, Transport,
};
use std::borrow::Cow;
use std::sync::Arc;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, WindowBuilder};
use wry::{NewWindowResponse, RequestAsyncResponder, WebContext, WebViewBuilder};

enum UserEvent {
    Navigate(String),
    Script(String),
    Title(String),
    Command(Command),
}

/// Runs in every page. Server pages may only ask to return to the launcher.
const SHORTCUTS: &str = r#"window.addEventListener("keydown", (e) => {
  if (e.ctrlKey && e.shiftKey && e.code === "KeyO") {
    e.preventDefault();
    window.ipc.postMessage('{"cmd":"show_servers"}');
  }
}, true);"#;

fn main() {
    if let Err(e) = run() {
        eprintln!("minerva: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = dirs::config_dir()
        .ok_or("No configuration directory is available")?
        .join("minerva");
    let data = dirs::data_local_dir()
        .ok_or("No local data directory is available")?
        .join("minerva");
    let registry = Registry::load(config.join("connections.json"))?;
    // `memory` keeps sign-ins only until exit, for kiosks, tests, and systems
    // without a credential service.
    let gateway = Arc::new(match std::env::var("MINERVA_CREDENTIAL_STORE").as_deref() {
        Ok("memory") => Gateway::new(Transport::new()?, registry, MemoryStore::default()),
        _ => Gateway::new(
            Transport::new()?,
            registry,
            KeyringStore::new("minerva-desktop"),
        ),
    });
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?,
    );
    std::fs::create_dir_all(&data)?;
    let local = Arc::new(LocalServer::open(&data.join("local.db"))?);
    let worker = SyncWorker::new(local.clone(), gateway.clone());
    runtime.spawn(worker.clone().run());

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title("Minerva")
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(LogicalSize::new(640.0, 480.0))
        .with_window_icon(Some(icon()))
        // Shown by `chrome::install` once the title bar is in place.
        .with_visible(false)
        .build(&event_loop)?;

    let chrome = chrome::install(&window, proxy.clone())?;

    let mut context = WebContext::new(Some(data.join("webview")));
    let builder = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(origin::page_url(origin::LOCAL, "/"))
        .with_asynchronous_custom_protocol(origin::SCHEME.into(), {
            let gateway = gateway.clone();
            let local = local.clone();
            let worker = worker.clone();
            let runtime = runtime.clone();
            move |_, request, responder| {
                serve(&gateway, &local, &worker, &runtime, request, responder)
            }
        })
        .with_ipc_handler({
            let gateway = gateway.clone();
            let worker = worker.clone();
            let runtime = runtime.clone();
            let proxy = proxy.clone();
            move |request: Request<String>| {
                ipc_message(&gateway, &worker, &runtime, &proxy, request)
            }
        })
        .with_navigation_handler({
            let gateway = gateway.clone();
            move |url| allow_navigation(&gateway, &url)
        })
        .with_new_window_req_handler(|url, _| {
            open_external(&url);
            NewWindowResponse::Deny
        })
        .with_document_title_changed_handler({
            let proxy = proxy.clone();
            move |title| {
                let _ = proxy.send_event(UserEvent::Title(title));
            }
        })
        // Attachments save to the Downloads folder without overwriting existing
        // files; WebKitGTK needs a handler registered to choose a destination.
        .with_download_started_handler(|_, _| true)
        .with_download_completed_handler({
            let gateway = gateway.clone();
            let local = local.clone();
            let runtime = runtime.clone();
            let proxy = proxy.clone();
            move |url, _, succeeded| {
                if !succeeded && origin::page_host(&url).is_some() {
                    let (gateway, local, proxy) = (gateway.clone(), local.clone(), proxy.clone());
                    runtime.spawn(async move {
                        let message = match downloads::save(&gateway, &local, &url).await {
                            Ok(path) => format!("Saved {}", path.display()),
                            Err(e) => format!("Download failed: {e}"),
                        };
                        let _ = proxy.send_event(UserEvent::Script(downloads::notice(&message)));
                    });
                }
            }
        })
        .with_initialization_script(SHORTCUTS)
        .with_devtools(cfg!(debug_assertions));
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_https_scheme(true)
    };
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().ok_or("Missing GTK container")?)?
    };
    #[cfg(target_os = "linux")]
    {
        print::install(&webview);
        media::install(&webview);
    }
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        // Owned by the loop so they live as long as the webview.
        let _ = (&context, &chrome, &runtime);
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            // Returning to the app is a good moment to sync, for example after
            // the network came back.
            Event::WindowEvent {
                event: WindowEvent::Focused(true),
                ..
            } => worker.wake(),
            Event::UserEvent(UserEvent::Navigate(url)) => {
                let _ = webview.load_url(&url);
            }
            Event::UserEvent(UserEvent::Script(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::Title(title)) => {
                window.set_title(if title.trim().is_empty() {
                    "Minerva"
                } else {
                    &title
                });
            }
            Event::UserEvent(UserEvent::Command(command)) => match command {
                Command::Servers => {
                    let _ = webview.load_url(&origin::page_url(origin::LAUNCHER, "/"));
                }
                Command::Reload => {
                    let _ = webview.reload();
                }
                Command::Quit => *control_flow = ControlFlow::Exit,
            },
            _ => {}
        }
    });
}

fn serve(
    gateway: &Arc<Gateway>,
    local: &Arc<LocalServer>,
    worker: &Arc<SyncWorker>,
    runtime: &tokio::runtime::Runtime,
    request: Request<Vec<u8>>,
    responder: RequestAsyncResponder,
) {
    let host = request.uri().host().unwrap_or_default().to_string();
    if host == origin::LAUNCHER {
        return responder.respond(launcher_asset(request.uri().path()));
    }
    // This computer's page asks the app itself about servers.
    if host == origin::LOCAL && request.uri().path().starts_with(bridge::PREFIX) {
        let (gateway, worker) = (gateway.clone(), worker.clone());
        runtime.spawn(async move {
            responder.respond(bridge::handle(gateway, worker, request).await);
        });
        return;
    }
    if host == origin::LOCAL {
        let local = local.clone();
        runtime.spawn(async move {
            responder.respond(local.handle(&origin::origin(&host), request).await);
        });
        return;
    }
    let Some(id) = origin::connection_id(&host).map(String::from) else {
        return responder.respond(plain(StatusCode::NOT_FOUND, "Not found"));
    };
    let gateway = gateway.clone();
    runtime.spawn(async move {
        let response = gateway.handle(&id, &origin::origin(&host), request).await;
        responder.respond(response);
    });
}

fn launcher_asset(path: &str) -> Response<Cow<'static, [u8]>> {
    let (body, kind): (&'static str, &str) = match path {
        "/" | "/index.html" => (
            include_str!("../assets/launcher.html"),
            "text/html; charset=utf-8",
        ),
        "/launcher.js" => (
            include_str!("../assets/launcher.js"),
            "text/javascript; charset=utf-8",
        ),
        "/launcher.css" => (
            include_str!("../assets/launcher.css"),
            "text/css; charset=utf-8",
        ),
        _ => return plain(StatusCode::NOT_FOUND, "Not found"),
    };
    Response::builder()
        .header("content-type", kind)
        .header("content-security-policy", "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
        .header("cache-control", "no-store")
        .body(Cow::Borrowed(body.as_bytes()))
        .unwrap()
}
fn plain(status: StatusCode, text: &'static str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Cow::Borrowed(text.as_bytes()))
        .unwrap()
}

fn ipc_message(
    gateway: &Arc<Gateway>,
    worker: &Arc<SyncWorker>,
    runtime: &tokio::runtime::Runtime,
    proxy: &EventLoopProxy<UserEvent>,
    request: Request<String>,
) {
    let Ok(message) = serde_json::from_str::<ipc::Message>(request.body()) else {
        return;
    };
    let from_launcher =
        origin::page_host(&request.uri().to_string()).as_deref() == Some(origin::LAUNCHER);
    if message.cmd == "show_servers" {
        let _ = proxy.send_event(UserEvent::Navigate(origin::page_url(origin::LAUNCHER, "/")));
        return;
    }
    if !from_launcher {
        return;
    }
    let (gateway, worker) = (gateway.clone(), worker.clone());
    let proxy = proxy.clone();
    runtime.spawn(async move {
        let (reply, effect) = ipc::dispatch(&gateway, &worker, &message).await;
        let _ = proxy.send_event(UserEvent::Script(format!(
            "window.__minervaReply({}, {reply})",
            message.id
        )));
        if let ipc::Effect::Navigate(url) = effect {
            let _ = proxy.send_event(UserEvent::Navigate(url));
        }
    });
}

/// Keeps the webview on the launcher, this computer's server, and registered
/// servers; anything else opens in the system browser.
fn allow_navigation(gateway: &Gateway, url: &str) -> bool {
    match origin::page_host(url) {
        Some(host) if host == origin::LAUNCHER || host == origin::LOCAL => true,
        Some(host)
            if origin::connection_id(&host).is_some_and(|id| gateway.connection(id).is_some()) =>
        {
            true
        }
        Some(_) => false,
        None if url == "about:blank" => true,
        None => {
            open_external(url);
            false
        }
    }
}
fn open_external(url: &str) {
    if let Ok(parsed) = url::Url::parse(url) {
        if matches!(parsed.scheme(), "http" | "https" | "mailto") {
            let _ = open::that_detached(parsed.as_str());
        }
    }
}

/// The application icon (128×128 RGBA, generated from favicon.png).
fn icon() -> Icon {
    Icon::from_rgba(include_bytes!("../assets/icon-128.rgba").to_vec(), 128, 128)
        .expect("valid icon dimensions")
}
