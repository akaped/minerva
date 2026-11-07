//! One-shot import of a Notion page and its child pages. Credentials are used
//! only for this request, against Notion's fixed origin, without redirects.
use super::*;
use std::time::Duration;

const VERSION: &str = "2025-09-03";
const MAX_PAGES: usize = 200;
const MAX_BLOCKS: usize = 20_000;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;

struct Client {
    http: reqwest::Client,
    token: String,
    base: String,
    requests: usize,
}
impl Client {
    async fn get(&mut self, path: &str) -> Result<Value, ApiError> {
        self.requests += 1;
        if self.requests > 2000 {
            return Err(bad(
                "This Notion import is too large; import a smaller page",
            ));
        }
        for attempt in 0..4 {
            // Stay below the default three requests/second per connection.
            tokio::time::sleep(Duration::from_millis(350)).await;
            let mut r = self
                .http
                .get(format!("{}{path}", self.base))
                .bearer_auth(&self.token)
                .header("Notion-Version", VERSION)
                .send()
                .await
                .map_err(|_| bad("Could not reach Notion"))?;
            if r.status().as_u16() == 429 && attempt < 3 {
                let wait = r
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(2);
                if wait > 60 {
                    return Err(bad("Notion is busy; try the import again later"));
                }
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }
            if !r.status().is_success() {
                // Do not expose provider responses, which may contain credentials.
                return Err(bad("Notion refused the request; check the token and share the page with its connection"));
            }
            let mut body = Vec::new();
            while let Some(chunk) = r
                .chunk()
                .await
                .map_err(|_| bad("Could not read Notion's response"))?
            {
                if body.len() + chunk.len() > MAX_RESPONSE {
                    return Err(bad(
                        "This Notion import is too large; import a smaller page",
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            return serde_json::from_slice(&body)
                .map_err(|_| bad("Could not read Notion's response"));
        }
        Err(bad("Notion is busy; try the import again later"))
    }
    async fn children(&mut self, id: &str) -> Result<Vec<Value>, ApiError> {
        let mut out = vec![];
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        loop {
            let mut path = format!("blocks/{id}/children?page_size=100");
            if let Some(c) = &cursor {
                path.push_str("&start_cursor=");
                path.extend(url::form_urlencoded::byte_serialize(c.as_bytes()));
            }
            let page = self.get(&path).await?;
            let rows = page["results"]
                .as_array()
                .ok_or_else(|| bad("Could not read Notion's response"))?;
            if out.len() + rows.len() > MAX_BLOCKS {
                return Err(bad(
                    "This Notion import is too large; import a smaller page",
                ));
            }
            out.extend(rows.iter().cloned());
            if page["has_more"] != true {
                break;
            }
            let next = page["next_cursor"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad("Could not read Notion's response"))?
                .to_string();
            if !seen.insert(next.clone()) {
                return Err(bad("Could not read Notion's response"));
            }
            cursor = Some(next);
        }
        Ok(out)
    }
}

pub(crate) async fn endpoint(
    State(store): State<Shared>,
    headers: HeaderMap,
    axum::Json(v): axum::Json<Value>,
) -> Result<Response, ApiError> {
    if headers
        .get("x-minerva-client")
        .and_then(|h| h.to_str().ok())
        != Some("1")
    {
        return Err(bad("X-Minerva-Client: 1 is required"));
    }
    let folder = field(&v, "folder_id")?;
    let page = Uuid::parse_str(v["page_id"].as_str().unwrap_or(""))
        .map_err(|_| bad("Enter the Notion page ID"))?
        .to_string();
    let token = v["token"]
        .as_str()
        .filter(|t| !t.is_empty() && t.len() <= 500 && !t.contains(['\r', '\n']))
        .ok_or_else(|| bad("Enter a Notion connection token"))?
        .to_string();
    {
        let s = store.lock().map_err(|_| internal())?;
        let u = authenticate(&s.data, &headers)?;
        access(&s.data, &u, &folder, true)?;
    }
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| internal())?;
    let mut client = Client {
        http,
        token,
        base: "https://api.notion.com/v1/".into(),
        requests: 0,
    };
    let export = tokio::time::timeout(Duration::from_secs(900), collect(&mut client, page))
        .await
        .map_err(|_| bad("The Notion import timed out; import a smaller page"))??;
    tokio::task::spawn_blocking(move || {
        let mut s = store.lock().map_err(|_| internal())?;
        // Permissions can change while the remote pages are being fetched.
        let u = authenticate(&s.data, &headers)?;
        access(&s.data, &u, &folder, true)?;
        Ok(axum::Json(apply(&mut s, &u, &folder, export)?).into_response())
    })
    .await
    .map_err(|_| internal())?
}

async fn collect(client: &mut Client, root: String) -> Result<Export, ApiError> {
    let mut export = Export::default();
    let mut pages = std::collections::VecDeque::from([(root, vec![])]);
    let mut visited = HashSet::new();
    let mut blocks = 0;
    let mut bytes = 0;
    let mut unsupported = HashSet::new();
    while let Some((id, dir)) = pages.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        if visited.len() > MAX_PAGES || dir.len() > 7 {
            return Err(bad(
                "This Notion import is too large; import a smaller page",
            ));
        }
        let page = client.get(&format!("pages/{id}")).await?;
        let title = page["properties"]
            .as_object()
            .and_then(|p| p.values().find(|p| p["type"] == "title"))
            .map(|p| plain(&p["title"]))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "Untitled".into());
        let mut markdown = String::new();
        let mut stack: Vec<_> = client
            .children(&id)
            .await?
            .into_iter()
            .rev()
            .map(|b| (b, 0usize))
            .collect();
        while let Some((b, depth)) = stack.pop() {
            blocks += 1;
            if blocks > MAX_BLOCKS || depth > 16 {
                return Err(bad(
                    "This Notion import is too large; import a smaller page",
                ));
            }
            let kind = b["type"].as_str().unwrap_or("unsupported");
            let bid = b["id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(|| bad("Could not read Notion's response"))?
                .to_string();
            if kind == "child_page" {
                let name = b[kind]["title"].as_str().unwrap_or("Untitled");
                markdown.push_str(&format!("[{}]({bid}.md)\n\n", escape(name)));
                let mut child_dir = dir.clone();
                child_dir.push(title.clone());
                pages.push_back((bid, child_dir));
                continue;
            }
            let rendered = block(&b);
            if let Some(text) = rendered {
                markdown.push_str(&"  ".repeat(depth));
                markdown.push_str(&text);
                markdown.push_str("\n\n");
            } else {
                unsupported.insert(kind.to_string());
                markdown.push_str(&format!(
                    "[Notion: {}](https://www.notion.so/{})\n\n",
                    escape(kind),
                    bid.replace('-', "")
                ));
            }
            if b["has_children"] == true && kind != "child_database" {
                let children = client.children(&bid).await?;
                stack.extend(children.into_iter().rev().map(|b| (b, depth + 1)));
            }
        }
        bytes += markdown.len();
        if bytes > 20 * 1024 * 1024 {
            return Err(bad(
                "This Notion import is too large; import a smaller page",
            ));
        }
        let mut metadata = BTreeMap::new();
        metadata.insert("notion_id".into(), id.clone());
        metadata.insert(
            "notion_url".into(),
            format!("https://www.notion.so/{}", id.replace('-', "")),
        );
        export.pages.push(Page {
            dir,
            path: format!("{id}.md"),
            title,
            markdown,
            tags: vec![],
            metadata,
        });
    }
    if !unsupported.is_empty() {
        let mut kinds: Vec<_> = unsupported.into_iter().collect();
        kinds.sort();
        export.warnings.push(format!("Kept links to Notion for unsupported content: {}. Use a Notion export to import database tables and attached files.", kinds.join(", ")));
    }
    Ok(export)
}
fn plain(v: &Value) -> String {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            r["plain_text"]
                .as_str()
                .or_else(|| r["text"]["content"].as_str())
                .unwrap_or("")
        })
        .collect()
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('`', "\\`")
}
fn rich(v: &Value) -> String {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let raw = r["plain_text"]
                .as_str()
                .or_else(|| r["text"]["content"].as_str())
                .unwrap_or("");
            let mut s = escape(raw);
            for (key, mark) in [("bold", "**"), ("italic", "*"), ("strikethrough", "~~")] {
                if r["annotations"][key] == true {
                    s = format!("{mark}{s}{mark}");
                }
            }
            if r["annotations"]["code"] == true {
                let longest = raw.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let fence = "`".repeat(longest + 1);
                s = format!("{fence} {raw} {fence}");
            }
            if let Some(url) = r["href"]
                .as_str()
                .or_else(|| r["text"]["link"]["url"].as_str())
            {
                if let Ok(u) = url::Url::parse(url) {
                    if matches!(u.scheme(), "https" | "http" | "mailto") {
                        s = format!(
                            "[{s}]({})",
                            url.replace(' ', "%20")
                                .replace('(', "%28")
                                .replace(')', "%29")
                        );
                    }
                }
            }
            s
        })
        .collect()
}
fn block(b: &Value) -> Option<String> {
    let kind = b["type"].as_str()?;
    let v = &b[kind];
    let text = rich(&v["rich_text"]);
    Some(match kind {
        "paragraph" | "toggle" => text,
        "heading_1" => format!("# {text}"),
        "heading_2" => format!("## {text}"),
        "heading_3" => format!("### {text}"),
        "bulleted_list_item" => format!("- {text}"),
        "numbered_list_item" => format!("1. {text}"),
        "to_do" => format!(
            "- [{}] {text}",
            if v["checked"] == true { "x" } else { " " }
        ),
        "quote" | "callout" => format!("> {text}"),
        "divider" => "---".into(),
        "code" => {
            let raw = plain(&v["rich_text"]);
            let longest = raw.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let fence = "`".repeat(3.max(longest + 1));
            format!("{fence}\n{raw}\n{fence}")
        }
        "equation" => format!("$$\n{}\n$$", v["expression"].as_str().unwrap_or("")),
        "column_list" | "column" => String::new(),
        // Hosted files expire; keep an explicit source link rather than silently
        // importing an image that will be broken in an hour.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn follows_pagination_and_child_pages_without_persisting_the_token() {
        use axum::extract::{Path, Query};
        let root = "11111111-1111-4111-8111-111111111111";
        let child = "22222222-2222-4222-8222-222222222222";
        let router = Router::new()
            .route("/pages/{id}", get(move |Path(id): Path<String>, headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], "Bearer test-secret");
                assert_eq!(headers["notion-version"], VERSION);
                axum::Json(json!({"properties":{"Name":{"type":"title","title":[{"plain_text":if id == root {"Parent"} else {"Child"}}]}}}))
            }))
            .route("/blocks/{id}/children", get(move |Path(id): Path<String>, Query(q): Query<HashMap<String,String>>| async move {
                if id == root && !q.contains_key("start_cursor") {
                    axum::Json(json!({"results":[{"id":child,"type":"child_page","child_page":{"title":"Child"}}],"has_more":true,"next_cursor":"next cursor"}))
                } else {
                    if id == root { assert_eq!(q.get("start_cursor").unwrap(), "next cursor"); }
                    axum::Json(json!({"results":[{"id":"33333333-3333-4333-8333-333333333333","type":"paragraph","paragraph":{"rich_text":[{"plain_text":"Fetched text"}]}}],"has_more":false}))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut client = Client {
            http: reqwest::Client::new(),
            token: "test-secret".into(),
            base: format!("http://{address}/"),
            requests: 0,
        };
        let export = collect(&mut client, root.into()).await.unwrap();
        server.abort();
        assert_eq!(export.pages.len(), 2);
        assert_eq!(export.pages[0].title, "Parent");
        assert!(export.pages[0].markdown.contains("Fetched text"));
        assert!(export.pages[0].markdown.contains(&format!("{child}.md")));
        assert_eq!(export.pages[1].dir, vec!["Parent"]);
        assert_eq!(export.pages[1].metadata["notion_id"], child);
        assert!(!export
            .pages
            .iter()
            .any(|p| p.markdown.contains("test-secret")
                || p.metadata.values().any(|v| v.contains("test-secret"))));
    }
    #[test]
    fn preserves_tasks_formatting_and_code_and_flags_unsupported_content() {
        assert_eq!(block(&json!({"type":"to_do","to_do":{"checked":true,"rich_text":[{"plain_text":"done","annotations":{"bold":true}}]}})).unwrap(), "- [x] **done**");
        assert_eq!(
            block(&json!({"type":"code","code":{"rich_text":[{"plain_text":"x < 2"}]}})).unwrap(),
            "```\nx < 2\n```"
        );
        assert_eq!(
            rich(&json!([{"plain_text":"a`b", "annotations":{"code":true}}])),
            "`` a`b ``"
        );
        assert!(block(&json!({"type":"child_database"})).is_none());
        assert!(block(&json!({"type":"image"})).is_none());
        assert_eq!(
            rich(&json!([{"plain_text":"safe","href":"javascript:alert(1)"}])),
            "safe"
        );
    }
}
