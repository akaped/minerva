//! The web clipper browser extension (`clipper/`): it connects to a server,
//! lists the folders its person can add notes to, and saves clipped pages,
//! selections and links as new notes.
//!
//! Connecting follows the browser's extension sign-in flow
//! (`identity.launchWebAuthFlow`), like OAuth with PKCE:
//! 1. The extension opens `/clipper/connect?redirect_uri=…&state=…&challenge=…`,
//!    the web app, where its person signs in as usual and approves.
//! 2. The app calls `POST clipper/approve` with the session cookie; the server
//!    keeps a one-time code for two minutes and answers with the extension's
//!    redirect address carrying it. The browser hands that address back to the
//!    extension; no page ever loads it.
//! 3. The extension calls `POST clipper/token` with the code and its PKCE
//!    verifier, and gets a personal API token that can only clip: it may list
//!    folders and add notes, nothing else (see `tokens::guard`), and revoke
//!    itself (`DELETE clipper/token`) when the extension disconnects.
use super::*;

/// How long an approval waits for the extension to collect it.
const CODE_SECONDS: u64 = 120;
/// The most Markdown a clip may hold, as for `POST notes`.
const CLIP_LIMIT: usize = 200_000;

#[derive(Clone)]
pub(super) struct Approval {
    user_id: String,
    challenge: String,
    redirect_uri: String,
    browser: String,
    expires: u64,
}

/// Whether `uri` is where a browser hands sign-ins back to an extension:
/// `https://<id>.chromiumapp.org/…` (Chrome, Edge, Brave…) or
/// `https://<id>.extensions.allizom.org/…` (Firefox).
pub(super) fn extension_redirect(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    let host = url.host_str().unwrap_or("");
    let label = |suffix: &str| {
        host.strip_suffix(suffix).is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    };
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && (label(".chromiumapp.org") || label(".extensions.allizom.org"))
}
fn browser_name(v: &Value) -> String {
    match v["browser"].as_str() {
        Some("firefox") => "Firefox",
        Some("chrome") => "Chrome",
        Some("edge") => "Edge",
        _ => "browser",
    }
    .to_string()
}

/// The routes. `approve` needs a signed-in session (never a token); `token`
/// needs no sign-in, only the code; the others take the clipper's token.
pub(super) fn route(
    d: &mut Data,
    headers: &HeaderMap,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Option<Value>, ApiError> {
    match (method, parts) {
        ("POST", ["clipper", "token"]) => token(d, v).map(Some),
        // Disconnecting: the extension revokes its own token.
        ("DELETE", ["clipper", "token"]) => {
            authenticate(d, headers)?;
            if !tokens::revoke_clipper(d, headers) {
                return Err(forbidden());
            }
            Ok(Some(json!({"ok": true})))
        }
        ("POST", ["clipper", "approve"]) => {
            let u = authenticate(d, headers)?;
            if tokens::used(headers) {
                return Err(forbidden());
            }
            approve(d, &u, v).map(Some)
        }
        ("GET", ["clipper", "folders"]) => {
            let u = authenticate(d, headers)?;
            Ok(Some(folders(d, &u)))
        }
        ("POST", ["clipper", "clip"]) => {
            let u = authenticate(d, headers)?;
            clip(d, &u, v).map(Some)
        }
        (_, ["clipper", ..]) => Err(missing()),
        _ => Ok(None),
    }
}

fn approve(d: &mut Data, u: &User, v: &Value) -> Result<Value, ApiError> {
    let redirect_uri = field(v, "redirect_uri")?;
    if !extension_redirect(&redirect_uri) {
        return Err(bad("This is not a browser extension's sign-in address"));
    }
    let challenge = field(v, "challenge")?;
    // A base64url SHA-256, as PKCE's S256 sends it.
    if challenge.len() != 43
        || !challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(bad("Invalid challenge"));
    }
    let state = v["state"].as_str().unwrap_or("");
    if state.len() > 200 {
        return Err(bad("Invalid state"));
    }
    let code = oauth::secret();
    d.clipper_codes.retain(|_, a| a.expires > now());
    d.clipper_codes.insert(
        hash_token(&code),
        Approval {
            user_id: u.id.clone(),
            challenge,
            redirect_uri: redirect_uri.clone(),
            browser: browser_name(v),
            expires: now() + CODE_SECONDS,
        },
    );
    let mut back = url::Url::parse(&redirect_uri).map_err(|_| internal())?;
    back.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", state);
    Ok(json!({"redirect": back.as_str()}))
}

fn token(d: &mut Data, v: &Value) -> Result<Value, ApiError> {
    let refused = || {
        ApiError(
            StatusCode::BAD_REQUEST,
            "This approval is unknown, used or expired. Connect again.".into(),
        )
    };
    // A code works once, whatever happens next.
    let approval = d
        .clipper_codes
        .remove(&hash_token(v["code"].as_str().unwrap_or("")))
        .filter(|a| a.expires > now())
        .ok_or_else(refused)?;
    let verifier = v["verifier"].as_str().unwrap_or("");
    let computed = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        Sha256::digest(verifier.as_bytes()),
    );
    if verifier.len() < 43 || computed != approval.challenge {
        return Err(refused());
    }
    if v["redirect_uri"].as_str() != Some(approval.redirect_uri.as_str()) {
        return Err(refused());
    }
    let account = d
        .users
        .iter()
        .find(|a| a.user.id == approval.user_id && !a.user.disabled && !a.user.admin)
        .ok_or_else(refused)?;
    let user = json!({"name": account.user.name, "email": account.user.email});
    let (token, expires_at) = tokens::issue_clipper(d, &approval.user_id, &approval.browser)?;
    Ok(json!({"token": token, "expires_at": expires_at, "user": user}))
}

/// The folders `u` can add notes to, by workspace, with each folder's path.
fn folders(d: &Data, u: &User) -> Value {
    let workspaces: Vec<Value> = d
        .workspaces
        .iter()
        .filter(|w| tokens::within(u, &w.id))
        .filter_map(|w| {
            let mut folders: Vec<(String, &str)> = d
                .folders
                .iter()
                .filter(|f| f.workspace_id == w.id && access(d, u, &f.id, true).is_ok())
                .map(|f| (folder_path(d, f), f.id.as_str()))
                .collect();
            if folders.is_empty() {
                return None;
            }
            folders.sort_by_key(|(path, _)| path.to_lowercase());
            Some(json!({
                "id": w.id,
                "name": w.name,
                "personal": w.personal,
                "folders": folders.iter().map(|(path, id)| json!({"id": id, "path": path})).collect::<Vec<_>>(),
            }))
        })
        .collect();
    json!({"workspaces": workspaces})
}
/// "Parent / Child", at most 32 levels up.
fn folder_path(d: &Data, f: &Folder) -> String {
    let mut names = vec![f.name.clone()];
    let mut parent = f.parent_id.clone();
    while let Some(id) = parent.take() {
        if names.len() >= 32 {
            break;
        }
        if let Some(p) = d.folders.iter().find(|x| x.id == id) {
            names.push(p.name.clone());
            parent = p.parent_id.clone();
        }
    }
    names.reverse();
    names.join(" / ")
}

fn clip(d: &mut Data, u: &User, v: &Value) -> Result<Value, ApiError> {
    let folder_id = field(v, "folder_id")?;
    access(d, u, &folder_id, true)?;
    let title: String = v["title"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect();
    let title = if title.is_empty() {
        "Clipped page".to_string()
    } else {
        title
    };
    let markdown = v["markdown"].as_str().unwrap_or("");
    if markdown.len() > CLIP_LIMIT {
        return Err(bad("At most 200,000 bytes of Markdown"));
    }
    let source = v["url"].as_str().unwrap_or("").trim();
    let source = url::Url::parse(source)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or_else(|| bad("The page's address must start with http:// or https://"))?;
    let tags: Vec<String> = v["tags"]
        .as_array()
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .filter_map(normal_tag)
                .take(20)
                .collect()
        })
        .unwrap_or_default();
    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("source".to_string(), source.to_string());
    let n = Note {
        id: id(),
        folder_id,
        title,
        icon: "🔖".into(),
        blocks: mcp::markdown_blocks(markdown)?,
        revision: 1,
        updated_at: now(),
        favorite: false,
        trashed: false,
        archived: false,
        cover: String::new(),
        tags,
        journal: None,
        metadata,
        print_template: String::new(),
        record: None,
        folded: None,
    };
    d.notes.push(Arc::new(n.clone()));
    let address = std::env::var("MINERVA_PUBLIC_URL")
        .map(|p| format!("{}/#note/{}", p.trim_end_matches('/'), n.id))
        .ok();
    Ok(json!({"id": n.id, "title": n.title, "folder_id": n.folder_id, "address": address}))
}
/// A tag as notes keep them: letters, digits, `-`, `_`, `/`, with a letter.
fn normal_tag(tag: &str) -> Option<String> {
    let value = tag.trim().trim_start_matches('#').to_lowercase();
    (value.chars().count() <= 40
        && !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '/' | '-'))
        && value.chars().any(char::is_alphabetic))
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_extension_sign_in_addresses_are_accepted() {
        for good in [
            "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/",
            "https://abcdefghijklmnopabcdefghijklmnop.chromiumapp.org/clipper",
            "https://0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c.extensions.allizom.org/",
        ] {
            assert!(extension_redirect(good), "{good}");
        }
        for bad in [
            "http://abc.chromiumapp.org/",
            "https://chromiumapp.org/",
            "https://a.b.chromiumapp.org/",
            "https://evil.example/?x=.chromiumapp.org",
            "https://abc.chromiumapp.org.evil.example/",
            "https://abc.chromiumapp.org:8443/",
            "https://user@abc.chromiumapp.org/",
            "https://abc.chromiumapp.org/#x",
            "javascript:alert(1)",
            "",
        ] {
            assert!(!extension_redirect(bad), "{bad}");
        }
    }
    #[test]
    fn tags_follow_the_note_rules() {
        assert_eq!(normal_tag("#Reading"), Some("reading".into()));
        assert_eq!(normal_tag("web/clips"), Some("web/clips".into()));
        assert_eq!(normal_tag("2026"), None);
        assert_eq!(normal_tag("two words"), None);
    }
}
