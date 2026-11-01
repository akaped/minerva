//! YouTube videos in notes. YouTube refuses to play in a page that has no web
//! address of its own (error 153), and Minerva's pages are served from the
//! `minerva://` scheme on Linux and macOS. A small server on 127.0.0.1 serves
//! one page per video, `/youtube/<id>?start=<seconds>`, that holds the player
//! and tells the note around it when the video plays or pauses. It serves
//! nothing else.
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Starts the server and returns its address, like `http://127.0.0.1:41234`.
pub fn start(runtime: &tokio::runtime::Runtime) -> Option<String> {
    let listener = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).ok()?;
    listener.set_nonblocking(true).ok()?;
    let address = format!("http://{}", listener.local_addr().ok()?);
    let _guard = runtime.enter();
    let listener = TcpListener::from_std(listener).ok()?;
    runtime.spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0u8; 2048];
                let Ok(read) = stream.read(&mut buffer).await else { return };
                let request = String::from_utf8_lossy(&buffer[..read]);
                let target = request
                    .strip_prefix("GET ")
                    .and_then(|rest| rest.split(' ').next())
                    .unwrap_or("");
                let response = match page(target) {
                    Some(html) => format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nreferrer-policy: strict-origin-when-cross-origin\r\ncontent-security-policy: default-src 'none'; script-src 'unsafe-inline' https://www.youtube.com https://s.ytimg.com; frame-src https://www.youtube-nocookie.com https://www.youtube.com; style-src 'unsafe-inline'\r\nconnection: close\r\n\r\n{html}",
                        html.len()
                    ),
                    None => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
                };
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    Some(address)
}

/// The player page for `/youtube/<11-character id>?start=<seconds>`.
fn page(target: &str) -> Option<String> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let id = path.strip_prefix("/youtube/")?;
    if id.len() != 11
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let start: u32 = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("start="))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
        .min(86_400);
    Some(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>YouTube video</title><style>html,body,#player{{margin:0;width:100%;height:100%;background:#000;overflow:hidden}}</style></head><body><div id="player"></div><script>
const tell = (message) => parent.postMessage(Object.assign({{ minervaYouTube: true }}, message), "*");
let player;
window.onYouTubeIframeAPIReady = () => {{
  player = new YT.Player("player", {{
    host: "https://www.youtube-nocookie.com",
    videoId: "{id}",
    width: "100%",
    height: "100%",
    playerVars: {{ playsinline: 1, start: {start}, origin: location.origin }},
    events: {{
      onStateChange: (e) => tell({{ state: e.data }}),
      onError: (e) => tell({{ error: e.data }}),
    }},
  }});
}};
addEventListener("message", (e) => {{
  if (e.source === parent && e.data && e.data.minervaYouTube === "pause") player?.pauseVideo?.();
}});
const script = document.createElement("script");
script.src = "https://www.youtube.com/iframe_api";
script.onerror = () => tell({{ error: "load" }});
document.head.append(script);
</script></body></html>"#
    ))
}

/// Pages the webview may load in frames for a video: the player page on
/// `local` (this server's address) and YouTube's own player.
pub fn allowed_frame(url: &str, local: Option<&str>) -> bool {
    if local.is_some_and(|base| {
        url.strip_prefix(base)
            .is_some_and(|rest| rest.starts_with("/youtube/"))
    }) {
        return true;
    }
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some(
                "www.youtube.com"
                    | "youtube.com"
                    | "www.youtube-nocookie.com"
                    | "youtube-nocookie.com"
            )
        )
        && url.path().starts_with("/embed/")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serves_only_video_pages() {
        assert!(page("/youtube/dQw4w9WgXcQ?start=42")
            .unwrap()
            .contains(r#"videoId: "dQw4w9WgXcQ""#));
        assert!(page("/youtube/dQw4w9WgXcQ?start=42")
            .unwrap()
            .contains("start: 42"));
        assert!(page("/youtube/too-short").is_none());
        assert!(page("/youtube/dQw4w9WgXc<").is_none());
        assert!(page("/").is_none());
    }
    #[test]
    fn allows_only_video_frames() {
        let base = Some("http://127.0.0.1:4000");
        assert!(allowed_frame(
            "http://127.0.0.1:4000/youtube/dQw4w9WgXcQ",
            base
        ));
        assert!(allowed_frame(
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?enablejsapi=1",
            base
        ));
        assert!(!allowed_frame("http://127.0.0.1:4000/other", base));
        assert!(!allowed_frame("http://127.0.0.1:5000/youtube/x", base));
        assert!(!allowed_frame(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            base
        ));
        assert!(!allowed_frame("https://evil.example/embed/x", base));
    }
}
