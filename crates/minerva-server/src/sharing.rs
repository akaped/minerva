//! Note sharing and publishing.
//!
//! - Member shares grant one note to a user on this server (viewer or editor).
//! - Link shares grant one note to anyone holding a secret token. Only the token's
//!   hash is stored; guests exchange it for an HttpOnly cookie scoped to the
//!   share API, so it never needs to appear in a URL path or query.
//! - Publications render a note as a public, read-only page under `/shared/{slug}` or a
//!   site's path (`/blog/{slug}`, see `sites`), either pinned to a snapshot or
//!   following live edits.
use super::*;
use crate::render::{render_for, Links};
use axum::extract::Path;

const SHARE_COOKIE: &str = "minerva_share";

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Share {
    pub id: String,
    pub note_id: String,
    /// `link` or `member`.
    pub kind: String,
    pub role: Role,
    #[serde(default)]
    pub token_hash: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    pub created_by: String,
    pub created_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub title: String,
    pub icon: String,
    #[serde(default)]
    pub cover: String,
    pub blocks: Vec<Block>,
    pub revision: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Publication {
    pub note_id: String,
    /// The site the page belongs to; empty for `/shared/`.
    #[serde(default)]
    pub site_id: String,
    pub slug: String,
    /// `pinned` (a snapshot taken when publishing) or `live` (follows edits).
    pub mode: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub snapshot: Option<Snapshot>,
    pub published_at: u64,
    pub updated_at: u64,
    pub published_by: String,
}

/// Role granted by a direct member share. Trashed notes are not shared.
pub(super) fn member_role(d: &Data, user_id: &str, n: &Note) -> Option<Role> {
    if n.trashed {
        return None;
    }
    let roles: Vec<&Role> = d
        .shares
        .iter()
        .filter(|s| {
            s.note_id == n.id && s.kind == "member" && s.user_id.as_deref() == Some(user_id)
        })
        .map(|s| &s.role)
        .collect();
    if roles.contains(&&Role::Editor) {
        Some(Role::Editor)
    } else {
        roles.first().map(|r| (*r).clone())
    }
}
/// Only folder-level editors (including workspace owners and administrators) manage
/// sharing, so a share recipient cannot re-share.
fn manage(d: &Data, u: &User, n: &Note) -> Result<(), ApiError> {
    if role(d, u, &n.folder_id) == Some(Role::Editor) {
        Ok(())
    } else {
        Err(forbidden())
    }
}
fn find_note<'a>(d: &'a Data, nid: &str) -> Result<&'a Note, ApiError> {
    d.notes.iter().find(|n| n.id == nid).ok_or_else(missing)
}
fn parse_role(v: &Value) -> Result<Role, ApiError> {
    match v["role"].as_str().unwrap_or("viewer") {
        "viewer" => Ok(Role::Viewer),
        "editor" => Ok(Role::Editor),
        _ => Err(bad("Choose view or edit access")),
    }
}

/// Indicator shown with note metadata to everyone who can see the note.
pub(super) fn summary(d: &Data, note_id: &str) -> Value {
    let count = |kind: &str| {
        d.shares
            .iter()
            .filter(|s| s.note_id == note_id && s.kind == kind)
            .count()
    };
    json!({
        "links": count("link"),
        "members": count("member"),
        "published": d.publications.iter().find(|p| p.note_id == note_id).map(|p| page_path(d, p)),
    })
}

fn share_view(d: &Data, s: &Share) -> Value {
    let user = s
        .user_id
        .as_ref()
        .and_then(|uid| d.users.iter().find(|a| &a.user.id == uid))
        .map(|a| json!({"id":a.user.id,"name":a.user.name,"email":a.user.email}));
    let creator = d
        .users
        .iter()
        .find(|a| a.user.id == s.created_by)
        .map(|a| &a.user.name);
    json!({"id":s.id,"kind":s.kind,"role":s.role,"created_at":s.created_at,"created_by":creator,"user":user})
}
/// A publication's address without the leading slash, such as `blog/hello`.
fn page_path(d: &Data, p: &Publication) -> String {
    format!("{}/{}", sites::base(d, &p.site_id), p.slug)
}
fn publication_view(d: &Data, p: &Publication) -> Value {
    json!({"slug":p.slug,"site_id":p.site_id,"path":page_path(d, p),"mode":p.mode,"description":p.description,"published_at":p.published_at,"updated_at":p.updated_at,"revision":p.snapshot.as_ref().map(|s| s.revision)})
}
pub(super) fn valid_slug(slug: &str) -> bool {
    (1..=80).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
}

/// Authenticated sharing routes. Returns `None` when the path is not a sharing route.
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Option<Value>, ApiError> {
    let result = match (method, parts) {
        ("GET", ["sharing"]) => overview(d, u),
        ("GET", ["notes", nid, "shares"]) => {
            let n = find_note(d, nid)?;
            manage(d, u, n)?;
            json!({
                "shares": d.shares.iter().filter(|s| s.note_id == n.id).map(|s| share_view(d, s)).collect::<Vec<_>>(),
                "publication": d.publications.iter().find(|p| p.note_id == n.id).map(|p| publication_view(d, p)),
            })
        }
        ("POST", ["notes", nid, "shares"]) => {
            let n = find_note(d, nid)?.clone();
            manage(d, u, &n)?;
            if n.trashed {
                return Err(bad("Restore this note before sharing it"));
            }
            if d.shares.iter().filter(|s| s.note_id == n.id).count() >= 50 {
                return Err(bad("A note can have up to 50 shares"));
            }
            let role = parse_role(v)?;
            match v["kind"].as_str() {
                Some("link") => {
                    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
                    let share = Share {
                        id: id(),
                        note_id: n.id.clone(),
                        kind: "link".into(),
                        role,
                        token_hash: Some(hash_token(&token)),
                        user_id: None,
                        created_by: u.id.clone(),
                        created_at: now(),
                    };
                    d.shares.push(share.clone());
                    // The raw token is returned once and never stored.
                    json!({"share":share_view(d,&share),"token":token})
                }
                Some("member") => {
                    let email = field(v, "email")?.to_lowercase();
                    let member = d
                        .users
                        .iter()
                        .find(|a| a.user.email == email && !a.user.disabled)
                        .map(|a| a.user.id.clone())
                        .ok_or_else(|| {
                            ApiError(
                                StatusCode::NOT_FOUND,
                                "No active member on this server uses that email".into(),
                            )
                        })?;
                    d.shares.retain(|s| {
                        !(s.note_id == n.id
                            && s.kind == "member"
                            && s.user_id.as_ref() == Some(&member))
                    });
                    let share = Share {
                        id: id(),
                        note_id: n.id.clone(),
                        kind: "member".into(),
                        role,
                        token_hash: None,
                        user_id: Some(member),
                        created_by: u.id.clone(),
                        created_at: now(),
                    };
                    d.shares.push(share.clone());
                    json!({"share":share_view(d,&share)})
                }
                _ => return Err(bad("Choose a link or a member")),
            }
        }
        ("DELETE", ["notes", nid, "shares", sid]) => {
            let n = find_note(d, nid)?;
            manage(d, u, n)?;
            let before = d.shares.len();
            d.shares.retain(|s| !(s.id == *sid && s.note_id == *nid));
            if d.shares.len() == before {
                return Err(missing());
            }
            json!({"ok":true})
        }
        ("PUT", ["notes", nid, "publication"]) => {
            let n = find_note(d, nid)?.clone();
            manage(d, u, &n)?;
            if n.trashed {
                return Err(bad("Restore this note before publishing it"));
            }
            let slug = v["slug"].as_str().unwrap_or("").trim().to_string();
            if !valid_slug(&slug) {
                return Err(bad(
                    "Addresses use 1–80 lowercase letters, digits, and inner hyphens",
                ));
            }
            let existing = d.publications.iter().find(|p| p.note_id == n.id).cloned();
            // Leaving out `site_id` keeps the page where it is.
            let site_id = match v.get("site_id") {
                Some(site) => site.as_str().unwrap_or("").trim().to_string(),
                None => existing
                    .as_ref()
                    .map(|p| p.site_id.clone())
                    .unwrap_or_default(),
            };
            sites::allowed(d, &n, &site_id)?;
            if d.publications
                .iter()
                .any(|p| p.site_id == site_id && p.slug == slug && p.note_id != n.id)
            {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "That address is already used by another page".into(),
                ));
            }
            let mode = match v["mode"].as_str().unwrap_or("pinned") {
                m @ ("pinned" | "live") => m.to_string(),
                _ => return Err(bad("Choose a pinned version or live updates")),
            };
            let description = v["description"].as_str().unwrap_or("").trim().to_string();
            if description.chars().count() > 300 {
                return Err(bad("Descriptions are limited to 300 characters"));
            }
            let refresh = v["refresh"].as_bool().unwrap_or(false);
            let snapshot = (mode == "pinned").then(|| match &existing {
                Some(Publication {
                    snapshot: Some(s), ..
                }) if !refresh => s.clone(),
                _ => Snapshot {
                    title: n.title.clone(),
                    icon: n.icon.clone(),
                    cover: n.cover.clone(),
                    blocks: n.blocks.clone(),
                    revision: n.revision,
                },
            });
            let publication = Publication {
                note_id: n.id.clone(),
                site_id,
                slug,
                mode,
                description,
                snapshot,
                published_at: existing
                    .as_ref()
                    .map(|p| p.published_at)
                    .unwrap_or_else(now),
                updated_at: now(),
                published_by: u.id.clone(),
            };
            d.publications.retain(|p| p.note_id != n.id);
            d.publications.push(publication.clone());
            publication_view(d, &publication)
        }
        ("DELETE", ["notes", nid, "publication"]) => {
            let n = find_note(d, nid)?;
            manage(d, u, n)?;
            d.publications.retain(|p| p.note_id != *nid);
            json!({"ok":true})
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

/// The Sharing page: notes `u` shares or publishes (with who and where), the
/// workspaces `u` shares and those shared with `u`.
fn overview(d: &Data, u: &User) -> Value {
    let workspace_of = |folder: &str| {
        d.folders
            .iter()
            .find(|f| f.id == folder)
            .and_then(|f| d.workspaces.iter().find(|w| w.id == f.workspace_id))
    };
    let name = |uid: &str| {
        d.users
            .iter()
            .find(|a| a.user.id == uid)
            .map_or_else(|| "Removed account".to_string(), |a| a.user.name.clone())
    };
    let managed: Vec<Value> = d
        .notes
        .iter()
        .filter(|n| role(d, u, &n.folder_id) == Some(Role::Editor))
        .filter(|n| d.shares.iter().any(|s| s.note_id == n.id) || d.publications.iter().any(|p| p.note_id == n.id))
        .map(|n| {
            let people: Vec<Value> = d
                .shares
                .iter()
                .filter(|s| s.note_id == n.id && s.kind == "member")
                .filter_map(|s| s.user_id.as_ref().map(|uid| json!({"name":name(uid),"role":s.role})))
                .collect();
            let w = workspace_of(&n.folder_id);
            json!({"id":n.id,"title":n.title,"icon":n.icon,"trashed":n.trashed,"updated_at":n.updated_at,
                "workspace_id":w.map(|w| &w.id),"workspace":w.map(|w| &w.name),
                "people":people,"sharing":summary(d,&n.id)})
        })
        .collect();
    let shared_with_me: Vec<Value> = shared_with(d, u)
        .into_iter()
        .map(|mut n| {
            let w = workspace_of(n["folder_id"].as_str().unwrap_or(""));
            n["workspace"] = json!(w.map(|w| &w.name));
            n
        })
        .collect();
    // Workspaces you own that have members, and those others share with you.
    let mut workspaces_shared = vec![];
    let mut workspaces_received = vec![];
    for w in d.workspaces.iter().filter(|w| !w.personal) {
        let members: Vec<Value> = d
            .workspace_grants
            .iter()
            .filter(|g| g.workspace_id == w.id)
            .map(|g| json!({"name":name(&g.user_id),"role":g.role}))
            .collect();
        if w.owner_id == u.id && !d.local {
            if !members.is_empty() {
                workspaces_shared.push(json!({"id":w.id,"name":w.name,"members":members}));
            }
        } else if let Some(g) = d
            .workspace_grants
            .iter()
            .find(|g| g.workspace_id == w.id && g.user_id == u.id)
        {
            workspaces_received
                .push(json!({"id":w.id,"name":w.name,"owner":name(&w.owner_id),"role":g.role}));
        }
    }
    json!({"managed":managed,"shared_with_me":shared_with_me,
        "workspaces_shared":workspaces_shared,"workspaces_received":workspaces_received})
}
/// Notes shared directly with `u` that their folder access would not already show.
pub(super) fn shared_with(d: &Data, u: &User) -> Vec<Value> {
    if u.admin && !d.local {
        return vec![];
    }
    d.notes
        .iter()
        .filter(|n| role(d, u, &n.folder_id).is_none())
        .filter_map(|n| {
            member_role(d, &u.id, n).map(|r| {
                json!({"id":n.id,"title":n.title,"icon":n.icon,"updated_at":n.updated_at,"role":r,"folder_id":n.folder_id,"favorite":n.favorite,"trashed":n.trashed,"tags":n.tags,"inline_tags":inline_tags(n),"sharing":summary(d,&n.id)})
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Share-link guests

/// What a link guest receives: the note content, never folder or workspace data.
pub(super) fn guest_view(n: &Note, role: &Role) -> Value {
    json!({
        "id": n.id,
        "title": n.title,
        "icon": n.icon,
        "blocks": n.blocks,
        "revision": n.revision,
        "updated_at": n.updated_at,
        "favorite": false,
        "trashed": false,
        "folder_id": "shared",
        "role": role,
        "cover": n.cover,
        "tags": n.tags,
        "inline_tags": inline_tags(n),
        "rendered": n.blocks.iter().map(|b| render_for(&b.markdown, Links::Shared)).collect::<Vec<_>>(),
    })
}
fn guest_share<'a>(d: &'a Data, headers: &HeaderMap) -> Result<(&'a Share, &'a Note), ApiError> {
    let gone = || {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "This link is no longer available".into(),
        )
    };
    let token = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|p| p.trim().strip_prefix(&format!("{SHARE_COOKIE}=")))
        })
        .ok_or_else(gone)?;
    link(d, token).ok_or_else(gone)
}
fn link<'a>(d: &'a Data, token: &str) -> Option<(&'a Share, &'a Note)> {
    let hash = hash_token(token);
    let share = d
        .shares
        .iter()
        .find(|s| s.kind == "link" && s.token_hash.as_deref() == Some(hash.as_str()))?;
    let note = d
        .notes
        .iter()
        .find(|n| n.id == share.note_id && !n.trashed)?;
    Some((share, note))
}
/// Whether a presence entry may still be shown: its share or member access remains.
pub(super) fn peer_allowed(d: &Data, user_id: &str, n: &Note) -> bool {
    if let Some(share) = user_id.strip_prefix("share:") {
        return !n.trashed && d.shares.iter().any(|s| s.id == share && s.note_id == n.id);
    }
    d.users
        .iter()
        .any(|a| a.user.id == user_id && !a.user.disabled && note_role(d, &a.user, n).is_some())
}

/// `/api/v1/shared/*`: the API available to share-link guests.
pub(super) async fn shared(
    State(store): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    tokio::task::spawn_blocking(move || {
        let mut s = store.lock().map_err(|_| internal())?;
        let path: Vec<&str> = uri.path().trim_start_matches("/api/v1/shared/").split('/').collect();
        if method != Method::GET && headers.get("x-minerva-client").and_then(|h| h.to_str().ok()) != Some("1") {
            return Err(bad("X-Minerva-Client: 1 is required"));
        }
        let json_body = || -> Result<Value, ApiError> {
            if body.is_empty() {
                Ok(json!({}))
            } else {
                serde_json::from_slice(&body).map_err(|_| bad("Invalid JSON"))
            }
        };
        if method == Method::POST && path == ["session"] {
            let token = json_body()?["token"].as_str().unwrap_or("").to_string();
            let (share, _) = link(&s.data, &token)
                .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "This link is no longer available".into()))?;
            let secure = if std::env::var("MINERVA_SECURE_COOKIE").as_deref() == Ok("1") { "; Secure" } else { "" };
            let mut response = axum::Json(json!({"role":share.role})).into_response();
            response.headers_mut().insert(
                "set-cookie",
                format!("{SHARE_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/api/v1/shared; Max-Age=86400{secure}")
                    .parse()
                    .map_err(|_| internal())?,
            );
            return Ok(response);
        }
        let (share, note) = guest_share(&s.data, &headers)?;
        let (share, note) = (share.clone(), note.clone());
        match (method.as_str(), path.as_slice()) {
            ("GET", ["note"]) => Ok(axum::Json(guest_view(&note, &share.role)).into_response()),
            ("POST", ["render"]) => {
                let v = json_body()?;
                let markdown = v["markdown"].as_str().ok_or_else(|| bad("Missing markdown"))?;
                if markdown.len() > 100_000 {
                    return Err(bad("Block is too large"));
                }
                Ok(axum::Json(json!({"html":render_for(markdown, Links::Shared)})).into_response())
            }
            ("POST", ["sync"]) => {
                let actor = collaboration::Actor {
                    id: format!("share:{}", share.id),
                    name: "Guest".into(),
                    role: Some(share.role.clone()),
                    guest: true,
                };
                let mut d = s.data.clone();
                let result = collaboration::sync(&mut d, &actor, &note.id, json_body()?)?;
                persist(&mut s, d)?;
                Ok(axum::Json(result).into_response())
            }
            ("POST", ["attachments"]) => {
                if share.role != Role::Editor {
                    return Err(forbidden());
                }
                attachments::upload(&s, &note, &uri, &body, None)
            }
            ("GET", ["attachments", key]) | ("GET", ["attachments", key, "preview"]) => {
                let preview = path.len() == 3;
                attachments::blob(&s, key, preview, |owner| {
                    if owner.id == note.id { Ok(()) } else { Err(forbidden()) }
                })
            }
            _ => Err(missing()),
        }
    })
    .await
    .map_err(|_| internal())?
}

// ---------------------------------------------------------------------------
// Published pages

/// What a published page shows.
struct Content {
    title: String,
    icon: String,
    cover: String,
    blocks: Vec<Block>,
    updated: u64,
}
/// The content a publication currently shows; `None` once the note is trashed.
fn published(d: &Data, p: &Publication) -> Option<Content> {
    let n = d.notes.iter().find(|n| n.id == p.note_id && !n.trashed)?;
    Some(match &p.snapshot {
        Some(s) if p.mode == "pinned" => Content {
            title: s.title.clone(),
            icon: s.icon.clone(),
            cover: s.cover.clone(),
            blocks: s.blocks.clone(),
            updated: p.updated_at,
        },
        _ => Content {
            title: n.title.clone(),
            icon: n.icon.clone(),
            cover: n.cover.clone(),
            blocks: n.blocks.clone(),
            updated: n.updated_at.max(p.updated_at),
        },
    })
}
/// Attachment IDs referenced by published or historical content, which cleanup must keep.
pub(super) fn published_text(d: &Data) -> impl Iterator<Item = &str> {
    d.publications.iter().flat_map(|p| {
        p.snapshot.iter().flat_map(|s| {
            s.blocks
                .iter()
                .map(|b| b.markdown.as_str())
                .chain(std::iter::once(s.cover.as_str()))
        })
    })
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn site_name() -> String {
    std::env::var("MINERVA_SITE_NAME").unwrap_or_else(|_| "Published notes".into())
}
pub(super) fn site_url(headers: &HeaderMap) -> String {
    if let Ok(url) = std::env::var("MINERVA_PUBLIC_URL") {
        return url.trim_end_matches('/').to_string();
    }
    let scheme = if std::env::var("MINERVA_SECURE_COOKIE").as_deref() == Ok("1") {
        "https"
    } else {
        "http"
    };
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .filter(|h| {
            h.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
        })
        .unwrap_or("localhost");
    format!("{scheme}://{host}")
}
pub(super) fn date(timestamp: u64) -> String {
    // Civil date from Unix days (Howard Hinnant's algorithm), avoiding a date dependency.
    let days = (timestamp / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
pub(super) fn rfc3339(timestamp: u64) -> String {
    let rem = timestamp % 86_400;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        date(timestamp),
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
/// One part of the public web: `/shared/` or a site such as `/blog/`.
struct Section {
    site_id: String,
    base: String,
    title: String,
    description: String,
    wiki: bool,
    theme: String,
}
impl Section {
    fn find(d: &Data, base: &str) -> Option<Section> {
        if base == sites::DEFAULT_BASE {
            return Some(Section {
                site_id: String::new(),
                base: sites::DEFAULT_BASE.into(),
                title: site_name(),
                description: String::new(),
                wiki: false,
                // Pages without a site use the server's default public theme.
                theme: std::env::var("MINERVA_PUBLIC_THEME").unwrap_or_default(),
            });
        }
        d.sites.iter().find(|s| s.path == base).map(|s| Section {
            site_id: s.id.clone(),
            base: s.path.clone(),
            title: s.title.clone(),
            description: s.description.clone(),
            wiki: s.layout == "wiki",
            theme: s.theme.clone(),
        })
    }
    /// Blogs offer an Atom feed; wikis do not.
    fn feed(&self) -> Option<String> {
        (!self.wiki).then(|| format!("/{}/feed.xml", self.base))
    }
}
fn page(
    title: &str,
    description: &str,
    body: &str,
    feed: Option<(&str, &str)>,
    theme: &str,
) -> Response {
    // A missing or removed theme falls back to the default look.
    let (theme_link, scheme) = if plugins::get(plugins::Kind::PublicThemes, theme).is_some() {
        let scheme = plugins::scheme(theme);
        (
            format!(
                "\n<link rel=\"stylesheet\" href=\"/plugins/public-notes-themes/{theme}.css\">"
            ),
            if scheme.is_empty() {
                String::new()
            } else {
                format!(" data-scheme=\"{scheme}\"")
            },
        )
    } else {
        (String::new(), String::new())
    };
    let feed = feed
        .map(|(name, href)| {
            format!(
                "\n<link rel=\"alternate\" type=\"application/atom+xml\" title=\"{}\" href=\"{}\">",
                escape(name),
                escape(href)
            )
        })
        .unwrap_or_default();
    let html = format!(
        r#"<!doctype html>
<html lang="en"{scheme}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<link rel="icon" href="/favicon.ico" sizes="any">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<link rel="stylesheet" href="/public.css">{theme_link}{feed}
</head>
<body>
{body}
</body>
</html>"#,
        title = escape(title),
        description = escape(description),
    );
    axum::response::Html(html).into_response()
}
fn not_found() -> Response {
    let mut response = page(
        "Page not found",
        "",
        r#"<main class="post"><h1>Page not found</h1><p>This page is not published.</p></main>"#,
        None,
        "",
    );
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}
/// Cover image or gradient for a page, with attachment URLs mapped for the audience.
fn cover_html(cover: &str, links: Links) -> String {
    if let Some(n) = cover.strip_prefix("gradient:") {
        return format!(
            "<div class=\"post-cover cover-gradient-{}\" aria-hidden=\"true\"></div>",
            escape(n)
        );
    }
    let src = match (render::attachment(cover), links) {
        (Some((id, _)), Links::Public(page)) => format!("/{page}/files/{id}/preview"),
        (Some((id, _)), _) => format!("/api/v1/attachments/{id}/preview"),
        (None, _) if cover.starts_with("https://") => cover.to_string(),
        _ => return String::new(),
    };
    format!(
        "<div class=\"post-cover\"><img src=\"{}\" alt=\"\"></div>",
        escape(&src)
    )
}
/// Points links to other notes (`#note/{id}`) at their published pages, so wiki
/// pages link to each other. Links to unpublished notes are dropped when rendering.
fn public_links(markdown: &str, pages: &[(String, String)]) -> String {
    if !markdown.contains("](#note/") {
        return markdown.to_string();
    }
    pages.iter().fold(markdown.to_string(), |text, (id, path)| {
        text.replace(&format!("](#note/{id})"), &format!("](/{path})"))
    })
}
fn rendered(blocks: &[Block], links: Links, pages: &[(String, String)]) -> Vec<String> {
    blocks
        .iter()
        .map(|b| render_for(&public_links(&b.markdown, pages), links))
        .collect()
}
fn article(
    c: &Content,
    links: Links,
    banner: &str,
    section: &Section,
    pages: &[(String, String)],
) -> String {
    let content: String = rendered(&c.blocks, links, pages)
        .iter()
        .map(|html| format!("<div class=\"block\">{html}</div>\n"))
        .collect();
    format!(
        r#"{banner}<header class="site"><a href="/{base}/">{site}</a></header>
{cover}<main class="post">
<article>
<div class="post-icon" aria-hidden="true">{icon}</div>
<h1>{title}</h1>
<p class="post-date">{label}<time datetime="{iso}">{date}</time></p>
{content}</article>
<nav class="post-nav"><a href="/{base}/">← All {kind}</a></nav>
</main>"#,
        base = escape(&section.base),
        site = escape(&section.title),
        cover = cover_html(&c.cover, links),
        icon = escape(&c.icon),
        title = escape(&c.title),
        label = if section.wiki { "Last updated " } else { "" },
        iso = rfc3339(c.updated),
        date = date(c.updated),
        kind = if section.wiki { "pages" } else { "posts" },
    )
}

/// `/{base}`, `/{base}/{slug}`, `/{base}/{slug}/files/{id}[/preview]`, and
/// `/{base}/feed.xml`, where `base` is `shared` or a site's path.
pub(super) async fn public(State(store): State<Shared>, uri: Uri, headers: HeaderMap) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Response, ApiError> {
        let s = store.lock().map_err(|_| internal())?;
        let d = &s.data;
        let segments: Vec<&str> = uri.path().split('/').filter(|p| !p.is_empty()).collect();
        let Some((base, path)) = segments.split_first() else {
            return Ok(not_found());
        };
        let Some(section) = Section::find(d, base) else {
            return Ok(not_found());
        };
        let live: Vec<(&Publication, Content)> = d
            .publications
            .iter()
            .filter_map(|p| published(d, p).map(|c| (p, c)))
            .collect();
        // Every live page, so links between published notes work across sites.
        let pages: Vec<(String, String)> = live
            .iter()
            .map(|(p, _)| (p.note_id.clone(), page_path(d, p)))
            .collect();
        let mut posts: Vec<&(&Publication, Content)> = live
            .iter()
            .filter(|(p, _)| p.site_id == section.site_id)
            .collect();
        if section.wiki {
            posts.sort_by_key(|(_, c)| c.title.to_lowercase());
        } else {
            posts.sort_by_key(|(p, _)| std::cmp::Reverse(p.published_at));
        }
        let feed = section.feed();
        let feed_link = feed.as_deref().map(|href| (section.title.as_str(), href));
        let b = escape(&section.base);
        match path {
            [] => {
                let items: String = posts
                    .iter()
                    .map(|(p, Content { title, .. })| {
                        format!(
                            "<li><a href=\"/{b}/{}\">{}</a>{}{}</li>\n",
                            escape(&p.slug),
                            escape(title),
                            if section.wiki { String::new() } else { format!("<time datetime=\"{}\">{}</time>", rfc3339(p.published_at), date(p.published_at)) },
                            if p.description.is_empty() { String::new() } else { format!("<p>{}</p>", escape(&p.description)) }
                        )
                    })
                    .collect();
                let body = format!(
                    "<main class=\"post index{}\"><h1>{}</h1>{}{}</main>",
                    if section.wiki { " wiki" } else { "" },
                    escape(&section.title),
                    if section.description.is_empty() { String::new() } else { format!("<p class=\"site-description\">{}</p>", escape(&section.description)) },
                    if items.is_empty() { "<p>Nothing has been published yet.</p>".into() } else { format!("<ul class=\"posts\">{items}</ul>") }
                );
                Ok(page(&section.title, &section.description, &body, feed_link, &section.theme))
            }
            ["feed.xml"] if feed.is_some() => {
                let base = site_url(&headers);
                let updated = posts.iter().map(|(_, c)| c.updated).max().unwrap_or(0);
                let entries: String = posts
                    .iter()
                    .take(50)
                    .map(|(p, c)| {
                        let page = page_path(d, p);
                        let content: String = rendered(&c.blocks, Links::Public(&page), &pages).concat();
                        format!(
                            "<entry><title>{}</title><link href=\"{base}/{url}\"/><id>{base}/{url}</id><published>{}</published><updated>{}</updated><summary>{}</summary><content type=\"html\">{}</content></entry>\n",
                            escape(&c.title),
                            rfc3339(p.published_at),
                            rfc3339(c.updated),
                            escape(&p.description),
                            escape(&content),
                            url = escape(&page),
                        )
                    })
                    .collect();
                let xml = format!(
                    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\" xml:base=\"{base}/\"><title>{}</title><id>{base}/{b}/</id><link href=\"{base}/{b}/\"/><link rel=\"self\" href=\"{base}/{b}/feed.xml\"/><updated>{}</updated>\n{entries}</feed>",
                    escape(&section.title),
                    rfc3339(updated),
                );
                Ok(([("content-type", "application/atom+xml; charset=utf-8")], xml).into_response())
            }
            [slug] => {
                let Some((p, content)) = posts.iter().find(|(p, _)| p.slug == *slug) else {
                    return Ok(not_found());
                };
                let description = if p.description.is_empty() { content.title.clone() } else { p.description.clone() };
                let page_url = page_path(d, p);
                Ok(page(&content.title, &description, &article(content, Links::Public(&page_url), "", &section, &pages), feed_link, &section.theme))
            }
            [slug, "files", key] | [slug, "files", key, "preview"] => {
                let Some((p, content)) = posts.iter().find(|(p, _)| p.slug == *slug) else {
                    return Ok(not_found());
                };
                // Only files of this note that the published content actually references.
                let referenced = content.cover.contains(*key) || content.blocks.iter().any(|b| b.markdown.contains(*key));
                let note_id = p.note_id.clone();
                attachments::blob(&s, key, path.len() == 4, |owner| {
                    if owner.id == note_id && referenced { Ok(()) } else { Err(missing()) }
                })
            }
            _ => Ok(not_found()),
        }
    })
    .await;
    match result {
        Ok(Ok(response)) => response,
        Ok(Err(ApiError(StatusCode::NOT_FOUND, _))) => not_found(),
        Ok(Err(e)) => e.into_response(),
        Err(_) => internal().into_response(),
    }
}

/// `/preview/{note_id}`: what publishing would show, for those who manage sharing.
pub(super) async fn preview(
    State(store): State<Shared>,
    Path(nid): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let s = store.lock().map_err(|_| internal())?;
    let u = authenticate(&s.data, &headers)?;
    let n = find_note(&s.data, &nid)?;
    manage(&s.data, &u, n)?;
    let banner = "<p class=\"preview-banner\" role=\"status\">Preview · This is how the page will look when published. It is not public until you publish it.</p>";
    // Preview in the site the note is published to, if any.
    let site_id = s
        .data
        .publications
        .iter()
        .find(|p| p.note_id == n.id)
        .map_or("", |p| p.site_id.as_str());
    let section = Section::find(&s.data, sites::base(&s.data, site_id)).ok_or_else(internal)?;
    Ok(page(
        &format!("Preview: {}", n.title),
        "",
        &article(
            &Content {
                title: n.title.clone(),
                icon: n.icon.clone(),
                cover: n.cover.clone(),
                blocks: n.blocks.clone(),
                updated: n.updated_at,
            },
            Links::Members,
            banner,
            &section,
            &[],
        ),
        None,
        &section.theme,
    ))
}

pub(super) const PUBLIC_CSS: &str = include_str!("../../../web/public.css");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slugs_and_dates() {
        assert!(valid_slug("hello-world-2"));
        assert!(!valid_slug("Hello"));
        assert!(!valid_slug("-a"));
        assert!(!valid_slug("a/b"));
        assert!(!valid_slug("feed.xml"));
        assert!(!valid_slug(""));
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(rfc3339(1_792_852_623), "2026-10-24T14:37:03Z");
    }
}

/// `/p/...`, where published pages used to live: a permanent redirect to the
/// same address under `/shared/`, so links already shared keep working.
pub(super) async fn moved(uri: Uri) -> Response {
    let rest = uri.path().strip_prefix("/p").unwrap_or("");
    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let target = format!(
        "/{}{}{query}",
        sites::DEFAULT_BASE,
        if rest.is_empty() { "/" } else { rest }
    );
    (
        axum::http::StatusCode::PERMANENT_REDIRECT,
        [(axum::http::header::LOCATION, target)],
    )
        .into_response()
}
