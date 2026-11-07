//! OpenRouter's native tools execute remotely. No provider response is executed
//! as a command on the Minerva host.
use super::*;

pub(super) const TOOLS: &[&str] = &[
    "openrouter:web_search",
    "openrouter:web_fetch",
    "openrouter:image_generation",
    "openrouter:datetime",
    "openrouter:fusion",
    "openrouter:advisor",
    "openrouter:subagent",
    "openrouter:shell",
    "minerva:shell",
];
pub(super) fn server_tools(v: &Value, kind: &str) -> Result<Vec<String>, ApiError> {
    let mut out = Vec::new();
    if let Some(value) = v.get("server_tools") {
        let list = value
            .as_array()
            .filter(|a| a.len() <= TOOLS.len())
            .ok_or_else(|| bad("Invalid server tools"))?;
        for item in list {
            let tool = item
                .as_str()
                .filter(|t| TOOLS.contains(t))
                .ok_or_else(|| bad("Invalid server tools"))?;
            if kind != "openrouter" {
                return Err(bad("Server tools require OpenRouter"));
            }
            if !out.iter().any(|t| t == tool) {
                out.push(tool.to_string());
            }
        }
    }
    if kind == "openrouter"
        && v["web_search"] == true
        && !out.iter().any(|t| t == "openrouter:web_search")
    {
        out.push("openrouter:web_search".into());
    }
    // Fusion internally enables both web tools, even if absent on the outer model.
    if out.iter().any(|t| t == "openrouter:fusion")
        && !["openrouter:web_search", "openrouter:web_fetch"]
            .iter()
            .all(|t| out.iter().any(|x| x == t))
    {
        return Err(bad("Fusion requires Web Search and Web Fetch"));
    }
    Ok(out)
}

async fn catalog(
    client: &reqwest::Client,
    endpoint: &str,
    key: &str,
    path: &str,
) -> Result<Value, ApiError> {
    let response = client
        .get(format!("{endpoint}/{path}"))
        .bearer_auth(key)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| bad("Provider discovery failed"))?;
    if !response.status().is_success() {
        return Err(bad("Provider discovery failed"));
    }
    let mut bytes = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| bad("Provider discovery failed"))?;
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err(bad("Provider catalog is too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| bad("Invalid provider catalog"))
}

pub(super) async fn discovery(
    store: &Shared,
    headers: &HeaderMap,
    v: &Value,
) -> Result<Value, ApiError> {
    let (client, endpoint, secret) = {
        let s = store.lock().map_err(|_| internal())?;
        let user = authenticate(&s.data, headers)?;
        admin(&user)?;
        if provider_kind(v)? != "openrouter" {
            return Err(bad("Model discovery requires OpenRouter"));
        }
        let endpoint = provider_endpoint(v)?;
        let secret = match provider_secret(v)? {
            Some(key) => key.to_string(),
            None => {
                let provider_id = v["provider_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| bad("An API key is required"))?;
                let (old, encrypted) = provider(&s, provider_id)?;
                // Even a changed path requires re-entry for discovery.
                if old.kind != "openrouter" || old.endpoint != endpoint {
                    return Err(bad(
                        "Enter the API key again to use it with a different server",
                    ));
                }
                s.ai.decrypt(&encrypted)?
            }
        };
        (s.ai.client.clone(), endpoint, secret)
    };
    let (models, tools) = tokio::join!(
        catalog(&client, &endpoint, &secret, "models"),
        catalog(&client, &endpoint, &secret, "tools")
    );
    let models = models?;
    let rows = models["data"]
        .as_array()
        .ok_or_else(|| bad("Invalid provider catalog"))?;
    let models: Vec<Value> = rows.iter().take(5000).filter_map(|m| {
        let id = m["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 200)?;
        Some(json!({"id":id,"name":m["name"].as_str().unwrap_or(id).chars().take(200).collect::<String>(),
            "context_length":m["context_length"].as_u64(),
            "input_modalities":m["architecture"]["input_modalities"],
            "output_modalities":m["architecture"]["output_modalities"],
            "supported_parameters":m["supported_parameters"], "native_tools":m["native_tools"]}))
    }).collect();
    let available = tools.as_ref().ok().and_then(|v| v["data"].as_array());
    let tools: Vec<Value> = available.into_iter().flatten().filter(|t| {
        t["id"].as_str().is_some_and(|id| TOOLS.contains(&id)) && t["status"] == "active" &&
            t["supported_api_formats"].as_array().is_some_and(|f| f.iter().any(|f| f == "responses"))
    }).map(|t| json!({"id":t["id"], "name":t["name"], "summary":t["summary"], "native_support":t["native_support"]})).collect();
    let s = store.lock().map_err(|_| internal())?;
    admin(&authenticate(&s.data, headers)?)?;
    Ok(json!({"models":models, "tools":tools, "tools_available":available.is_some()}))
}

/// Only bounded raster data URLs are embedded; SVG and arbitrary remote URLs
/// cannot turn a generated image into active content or a tracking request.
pub(super) fn image(item: &Value) -> Option<&str> {
    if item["type"] != "openrouter:image_generation" || item["status"] != "completed" {
        return None;
    }
    let url = item["imageUrl"].as_str().or(item["result"].as_str())?;
    let (prefix, data) = url.split_once(',')?;
    if ![
        "data:image/png;base64",
        "data:image/jpeg;base64",
        "data:image/webp;base64",
    ]
    .contains(&prefix)
        || data.is_empty()
        || url.len() > 10_000_000
    {
        return None;
    }
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_policy_is_explicit_and_fusion_requires_its_web_tools() {
        assert!(server_tools(&json!({}), "openrouter").unwrap().is_empty());
        assert!(
            server_tools(&json!({"server_tools":["openrouter:fusion"]}), "openrouter").is_err()
        );
        assert!(server_tools(
            &json!({"server_tools":["openrouter:datetime"]}),
            "compatible"
        )
        .is_err());
        assert!(server_tools(
            &json!({"server_tools":["openrouter:unknown"]}),
            "openrouter"
        )
        .is_err());
        assert_eq!(server_tools(&json!({"server_tools":["openrouter:fusion","openrouter:web_search","openrouter:web_fetch"]}),"openrouter").unwrap().len(),3);
    }
    #[test]
    fn images_reject_active_content_and_remote_urls() {
        for url in [
            "javascript:alert(1)",
            "https://tracker.example/image.png",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:image/png;base64,???",
        ] {
            assert!(image(
                &json!({"type":"openrouter:image_generation","status":"completed","imageUrl":url})
            )
            .is_none());
        }
    }
}
