//! Public sites: named paths such as `/blog` or `/wiki` that group published pages.
//!
//! - A site belongs to one workspace. Its owner (or an administrator) creates,
//!   renames, and removes it; any folder editor in that workspace can publish into it.
//! - Only notes from the site's workspace can be published there, so one workspace
//!   cannot place pages under another's path.
//! - Pages without a site live under `/shared/` (formerly `/p/`, which redirects).
//! - `blog` sites list pages newest first and offer an Atom feed; `wiki` sites list
//!   pages alphabetically and show when each page last changed.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Site {
    pub id: String,
    pub workspace_id: String,
    pub path: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// `blog` or `wiki`.
    pub layout: String,
    /// A public theme from `plugins/public-notes-themes`; empty for the default look.
    #[serde(default)]
    pub theme: String,
    pub created_at: u64,
}

/// Where published pages without a site live: `/shared/{slug}`. Pages that
/// were published under `/p/` are redirected there.
pub(super) const DEFAULT_BASE: &str = "shared";

/// First path segments the server already uses or may need, which sites cannot take.
const RESERVED: &[&str] = &[
    "p", "api", "export", "share", "shared", "preview", "admin", "app", "assets", "static",
    "files", "login", "logout", "settings", "account", "health", "plugins",
];

fn valid_path(path: &str) -> bool {
    sharing::valid_slug(path) && path.len() <= 40 && !RESERVED.contains(&path)
}
fn layout(v: &Value) -> Result<Option<String>, ApiError> {
    match v["layout"].as_str() {
        None => Ok(None),
        Some(l @ ("blog" | "wiki")) => Ok(Some(l.to_string())),
        Some(_) => Err(bad("Choose a blog or wiki layout")),
    }
}
fn theme(v: &Value) -> Result<Option<String>, ApiError> {
    match v["theme"].as_str().map(str::trim) {
        None => Ok(None),
        Some("") => Ok(Some(String::new())),
        Some(t) if plugins::get(plugins::Kind::PublicThemes, t).is_some() => Ok(Some(t.into())),
        Some(_) => Err(bad("That theme is not installed")),
    }
}
fn text(v: &Value, key: &str, max: usize) -> Result<Option<String>, ApiError> {
    match v[key].as_str().map(str::trim) {
        Some(t) if t.chars().count() > max => {
            Err(bad(&format!("The {key} is limited to {max} characters")))
        }
        t => Ok(t.map(str::to_string)),
    }
}
fn claim_path(d: &Data, path: &str, site_id: &str) -> Result<(), ApiError> {
    if !valid_path(path) {
        return Err(bad(
            "Site paths use up to 40 lowercase letters, digits, and inner hyphens, and cannot be a reserved name",
        ));
    }
    if d.sites.iter().any(|s| s.path == path && s.id != site_id) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Another site already uses that path".into(),
        ));
    }
    Ok(())
}
/// The site's path segment for a publication: `p` when it has no site.
pub(super) fn base<'a>(d: &'a Data, site_id: &str) -> &'a str {
    d.sites
        .iter()
        .find(|s| s.id == site_id)
        .map_or(DEFAULT_BASE, |s| s.path.as_str())
}
/// Checks that a note may be published into `site_id` (empty: `/shared/`).
pub(super) fn allowed(d: &Data, n: &Note, site_id: &str) -> Result<(), ApiError> {
    if site_id.is_empty() {
        return Ok(());
    }
    let site = d
        .sites
        .iter()
        .find(|s| s.id == site_id)
        .ok_or_else(missing)?;
    let workspace = d
        .folders
        .iter()
        .find(|f| f.id == n.folder_id)
        .map(|f| f.workspace_id.as_str());
    if workspace != Some(site.workspace_id.as_str()) {
        return Err(bad("Choose a site from this note's workspace"));
    }
    Ok(())
}
fn view(d: &Data, s: &Site) -> Value {
    let mut v = json!(s);
    v["pages"] = json!(d.publications.iter().filter(|p| p.site_id == s.id).count());
    v
}
/// Sites of the workspaces `visible` lists, for the bootstrap payload.
pub(super) fn listed(d: &Data, visible: &[&Workspace]) -> Vec<Value> {
    d.sites
        .iter()
        .filter(|s| visible.iter().any(|w| w.id == s.workspace_id))
        .map(|s| view(d, s))
        .collect()
}

/// `POST /sites`, `PATCH /sites/{id}`, and `DELETE /sites/{id}`.
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    match (method, parts) {
        ("POST", ["sites"]) => {
            let workspace_id = field(v, "workspace_id")?;
            if !d.workspaces.iter().any(|w| w.id == workspace_id) {
                return Err(missing());
            }
            if !edits_workspace(d, u, &workspace_id) {
                return Err(forbidden());
            }
            if d.sites
                .iter()
                .filter(|s| s.workspace_id == workspace_id)
                .count()
                >= 20
            {
                return Err(bad("A workspace can have up to 20 sites"));
            }
            let path = field(v, "path")?.trim().to_string();
            claim_path(d, &path, "")?;
            let site = Site {
                id: id(),
                workspace_id,
                title: text(v, "title", 100)?
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| path.clone()),
                path,
                description: text(v, "description", 300)?.unwrap_or_default(),
                layout: layout(v)?.unwrap_or_else(|| "blog".into()),
                theme: theme(v)?.unwrap_or_default(),
                created_at: now(),
            };
            d.sites.push(site.clone());
            Ok(view(d, &site))
        }
        ("PATCH", ["sites", sid]) => {
            let site = d.sites.iter().find(|s| s.id == *sid).ok_or_else(missing)?;
            if !edits_workspace(d, u, &site.workspace_id) {
                return Err(forbidden());
            }
            let path = text(v, "path", 40)?;
            if let Some(path) = &path {
                claim_path(d, path, sid)?;
            }
            let (title, description, layout, theme) = (
                text(v, "title", 100)?,
                text(v, "description", 300)?,
                layout(v)?,
                theme(v)?,
            );
            let site = d.sites.iter_mut().find(|s| s.id == *sid).unwrap();
            if let Some(path) = path {
                site.path = path;
            }
            if let Some(title) = title.filter(|t| !t.is_empty()) {
                site.title = title;
            }
            if let Some(description) = description {
                site.description = description;
            }
            if let Some(layout) = layout {
                site.layout = layout;
            }
            if let Some(theme) = theme {
                site.theme = theme;
            }
            let site = site.clone();
            Ok(view(d, &site))
        }
        ("DELETE", ["sites", sid]) => {
            let site = d.sites.iter().find(|s| s.id == *sid).ok_or_else(missing)?;
            if !edits_workspace(d, u, &site.workspace_id) {
                return Err(forbidden());
            }
            let pages = d.publications.iter().filter(|p| p.site_id == *sid).count();
            if pages > 0 {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    format!("Move or unpublish this site's {pages} page(s) first"),
                ));
            }
            d.sites.retain(|s| s.id != *sid);
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths() {
        assert!(valid_path("blog"));
        assert!(valid_path("team-wiki"));
        for bad in ["p", "api", "export", "Blog", "a/b", "", "-x", "feed.xml"] {
            assert!(!valid_path(bad), "{bad}");
        }
        assert!(!valid_path(&"a".repeat(41)));
    }
}
