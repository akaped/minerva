//! Link previews (`GET /api/v1/link-preview?url=…`): the title, description,
//! site name and picture a web page gives about itself, for the card a block
//! holding just that address shows. Only signed-in people may ask, and only for
//! public web addresses: the server resolves the name itself, refuses private,
//! loopback and other special addresses, connects to the address it checked
//! (so the name cannot be switched to another one meanwhile), checks every
//! redirect the same way, and reads at most 1 MB of HTML within a few seconds.
//! Answers are kept for a day.
use super::*;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

const BODY: usize = 1024 * 1024;
const REDIRECTS: usize = 3;
const KEPT: u64 = 86400;

fn cache() -> &'static Mutex<HashMap<String, (u64, Value)>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<String, (u64, Value)>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Whether the server may connect to `ip` for a preview: public addresses only.
pub(crate) fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (b == 18 || b == 19))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_address(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (first == 0x2001 && v6.segments()[1] == 0x0db8)
                || first == 0x0064)
        }
    }
}

/// The page's own description of itself, from its `<head>`: Open Graph tags
/// first, then the plain title and description.
pub(crate) fn page_details(html: &str, base: &url::Url) -> Value {
    let head = &html[..html.find("</head>").unwrap_or(html.len()).min(html.len())];
    let mut meta: HashMap<String, String> = HashMap::new();
    let mut rest = head;
    while let Some(at) = rest.find("<meta") {
        let tag_end = rest[at..].find('>').map_or(rest.len(), |e| at + e);
        let tag = &rest[at..tag_end];
        let attr = |name: &str| -> Option<String> {
            let lower = tag.to_lowercase();
            let mut from = 0;
            while let Some(i) = lower[from..].find(&format!("{name}=")).map(|i| i + from) {
                // A whole attribute name, not the end of another one.
                let before = lower[..i].chars().next_back();
                if before.is_some_and(|c| c.is_whitespace()) {
                    let value = &tag[i + name.len() + 1..];
                    let quote = value.chars().next()?;
                    return Some(if quote == '"' || quote == '\'' {
                        value[1..].split(quote).next()?.to_string()
                    } else {
                        value.split([' ', '/', '>']).next()?.to_string()
                    });
                }
                from = i + 1;
            }
            None
        };
        if let (Some(key), Some(content)) =
            (attr("property").or_else(|| attr("name")), attr("content"))
        {
            meta.entry(key.to_lowercase()).or_insert(content);
        }
        rest = &rest[tag_end..];
    }
    let title_tag = head.find("<title").and_then(|at| {
        let start = head[at..].find('>')? + at + 1;
        let end = head[start..].find("</title>")? + start;
        Some(head[start..end].to_string())
    });
    let clean = |s: &str, max: usize| -> String {
        let text = decode_entities(s);
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        text.chars().take(max).collect()
    };
    let pick = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| meta.get(*k).filter(|v| !v.trim().is_empty()))
    };
    let title = pick(&["og:title", "twitter:title"])
        .cloned()
        .or(title_tag)
        .map(|t| clean(&t, 200));
    let description =
        pick(&["og:description", "twitter:description", "description"]).map(|d| clean(d, 300));
    let site = pick(&["og:site_name"]).map(|s| clean(s, 80));
    let image = pick(&["og:image", "og:image:url", "twitter:image"])
        .and_then(|i| base.join(&decode_entities(i)).ok())
        .filter(|u| u.scheme() == "https" || u.scheme() == "http")
        .map(|u| u.to_string());
    json!({
        "url": base.as_str(),
        "title": title,
        "description": description,
        "site": site.unwrap_or_else(|| base.host_str().unwrap_or("").trim_start_matches("www.").to_string()),
        "image": image,
    })
}
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// Fetches a page by way of addresses checked to be public, following up to a
/// few redirects, each checked again.
async fn fetch(start: url::Url) -> Result<(url::Url, String), ApiError> {
    let refused = || bad("This address cannot be previewed");
    let mut url = start;
    for _ in 0..=REDIRECTS {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(refused());
        }
        let host = url.host_str().ok_or_else(refused)?.to_string();
        let port = url.port_or_known_default().ok_or_else(refused)?;
        if !matches!(port, 80 | 443) {
            return Err(refused());
        }
        let address: SocketAddr = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::lookup_host((host.as_str(), port)),
        )
        .await
        .map_err(|_| refused())?
        .map_err(|_| refused())?
        .next()
        .ok_or_else(refused)?;
        if !public_address(address.ip()) {
            return Err(refused());
        }
        let client = reqwest::Client::builder()
            .resolve(&host, address)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .user_agent("Minerva link preview")
            .build()
            .map_err(|_| internal())?;
        let response = client
            .get(url.clone())
            .header("accept", "text/html")
            .send()
            .await
            .map_err(|_| bad("The page did not answer"))?;
        if response.status().is_redirection() {
            let next = response
                .headers()
                .get("location")
                .and_then(|l| l.to_str().ok())
                .and_then(|l| url.join(l).ok())
                .ok_or_else(refused)?;
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(bad("The page could not be read"));
        }
        let html = response
            .headers()
            .get("content-type")
            .and_then(|t| t.to_str().ok())
            .is_some_and(|t| t.contains("text/html") || t.contains("xhtml"));
        if !html {
            return Err(bad("This address is not a web page"));
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
            let chunk = chunk.map_err(|_| bad("The page could not be read"))?;
            body.extend_from_slice(&chunk);
            if body.len() >= BODY {
                break;
            }
        }
        return Ok((url, String::from_utf8_lossy(&body).into_owned()));
    }
    Err(bad("The page redirects too many times"))
}

pub(super) async fn endpoint(
    State(store): State<Shared>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    {
        let s = store.lock().map_err(|_| internal())?;
        authenticate(&s.data, &headers)?;
    }
    let raw = url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    let url = url::Url::parse(raw.trim()).map_err(|_| bad("Give a web address"))?;
    if raw.len() > 2048 || !matches!(url.scheme(), "http" | "https") {
        return Err(bad("Give a web address"));
    }
    let key = url.to_string();
    if let Some((at, value)) = cache().lock().map_err(|_| internal())?.get(&key) {
        if at + KEPT > now() {
            return Ok(axum::Json(value.clone()).into_response());
        }
    }
    let (final_url, html) = fetch(url).await?;
    let mut details = page_details(&html, &final_url);
    details["url"] = json!(key);
    let mut kept = cache().lock().map_err(|_| internal())?;
    if kept.len() >= 1000 {
        kept.retain(|_, (at, _)| *at + KEPT > now());
        if kept.len() >= 1000 {
            kept.clear();
        }
    }
    kept.insert(key, (now(), details.clone()));
    Ok(axum::Json(details).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_public_addresses() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "224.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!public_address(private.parse().unwrap()), "{private}");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_address(public.parse().unwrap()), "{public}");
        }
    }
    #[test]
    fn details_from_the_page_head() {
        let base = url::Url::parse("https://www.example.com/post/1").unwrap();
        let html = r#"<html><head><title>Plain title</title>
            <meta name="description" content="Plain description">
            <meta property="og:title" content="Rich &amp; title">
            <meta content="/img/card.png" property="og:image">
            <meta property="og:site_name" content="Example">
            </head><body><meta property="og:title" content="Not this"></body></html>"#;
        let d = page_details(html, &base);
        assert_eq!(d["title"], "Rich & title");
        assert_eq!(d["description"], "Plain description");
        assert_eq!(d["site"], "Example");
        assert_eq!(d["image"], "https://www.example.com/img/card.png");
        let bare = page_details("<title>Just  a\n title</title>", &base);
        assert_eq!(bare["title"], "Just a title");
        assert_eq!(bare["site"], "example.com");
        assert!(bare["image"].is_null());
    }
}
