//! Attachment downloads. WebKitGTK cannot download from custom URI schemes, so a
//! failed download of a connection URL is fetched through the gateway (or from
//! the local server) instead and written to the user's Downloads folder.
use crate::origin;
use http::Request;
use minerva_client::{Gateway, LocalServer};
use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};

/// Downloads `url` natively and returns the saved path.
pub async fn save(gateway: &Gateway, local: &LocalServer, url: &str) -> Result<PathBuf, String> {
    let host = origin::page_host(url).ok_or("Not a Minerva URL")?;
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    let request = Request::get(format!(
        "{}://{host}{}",
        origin::SCHEME,
        &parsed[url::Position::BeforePath..]
    ))
    .body(Vec::new())
    .map_err(|e| e.to_string())?;
    let response = if host == origin::LOCAL {
        local.handle(&origin::origin(&host), request).await
    } else {
        let id = origin::connection_id(&host).ok_or("Not a server URL")?;
        gateway.handle(id, &origin::origin(&host), request).await
    };
    if !response.status().is_success() {
        return Err(format!(
            "The server answered HTTP {}",
            response.status().as_u16()
        ));
    }
    let name = response
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(disposition_filename)
        .or_else(|| {
            parsed
                .path_segments()
                .and_then(|mut s| s.next_back())
                .map(String::from)
        })
        .unwrap_or_default();
    let folder = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or("No Downloads folder is available")?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = unique(&folder, &sanitize(&name));
    std::fs::write(&path, response.body()).map_err(|e| e.to_string())?;
    Ok(path)
}

fn disposition_filename(header: &str) -> Option<String> {
    let mut plain = None;
    for part in header.split(';').map(str::trim) {
        if let Some(value) = part.strip_prefix("filename*=") {
            let encoded = value.split_once("''").map(|(_, v)| v).unwrap_or(value);
            return Some(percent_decode_str(encoded).decode_utf8_lossy().into_owned());
        }
        if let Some(value) = part.strip_prefix("filename=") {
            plain = Some(value.trim_matches('"').to_string());
        }
    }
    plain
}
/// Keeps only a plain file name: no directories, control characters, or reserved
/// Windows characters, and never a hidden or empty name.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        "download".into()
    } else {
        cleaned.chars().take(200).collect()
    }
}
fn unique(folder: &Path, name: &str) -> PathBuf {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let mut path = folder.join(name);
    let mut counter = 1;
    while path.exists() {
        path = folder.join(format!("{stem} ({counter}){extension}"));
        counter += 1;
    }
    path
}

/// Script showing a short notice in the current page.
pub fn notice(message: &str) -> String {
    let text = serde_json::to_string(message).unwrap_or_default();
    format!(
        r#"(() => {{
  const n = document.createElement("div");
  n.textContent = {text};
  n.setAttribute("role", "status");
  n.style.cssText = "position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:2147483647;padding:10px 16px;border-radius:8px;background:#263249;color:#fff;font:13px system-ui,sans-serif;box-shadow:0 6px 20px #0004";
  document.body.append(n);
  setTimeout(() => n.remove(), 5000);
}})()"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derives_safe_file_names() {
        assert_eq!(
            disposition_filename(
                "attachment; filename=\"a b.txt\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"
            )
            .as_deref(),
            Some("résumé.pdf")
        );
        assert_eq!(
            disposition_filename("attachment; filename=\"plain.txt\"").as_deref(),
            Some("plain.txt")
        );
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(sanitize(".bashrc"), "bashrc");
        assert_eq!(sanitize("con:\u{7}x?.txt"), "con__x_.txt");
        assert_eq!(sanitize(""), "download");
        assert_eq!(sanitize("..."), "download");
    }
    #[test]
    fn never_overwrites() {
        let folder = std::env::temp_dir().join(format!("minerva-dl-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.txt"), "x").unwrap();
        assert_eq!(unique(&folder, "a.txt"), folder.join("a (1).txt"));
        assert_eq!(unique(&folder, "b.txt"), folder.join("b.txt"));
        std::fs::remove_dir_all(folder).unwrap();
    }
}
