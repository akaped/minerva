//! Frontend plugins: app themes and public website themes.
//!
//! - `themes/{id}.css` are app color themes, applied on top of the built-in light
//!   or dark theme (the `base`) and scoped to `[data-variant="{id}"]`.
//! - `public-notes-themes/{id}.css` restyle published pages (`/shared/` and sites).
//! - `pdf-prints/{id}.css` are PDF templates; an optional `{id}.html` replaces the
//!   default page layout. See `plugins/README.md` for the variables they use.
//!
//! Built-in plugins are compiled into the server. A plugin folder
//! (`MINERVA_PLUGINS`, default `plugins`) adds more and overrides built-ins with
//! the same ID; it is read on each request, so edits apply without a restart.
//! Each file starts with a comment header of `key: value` lines (`name`,
//! `description`, and `base` or `scheme`).
use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Themes,
    PublicThemes,
    PdfPrints,
}
/// The built-in plugins of a folder, as `(id, stylesheet)`.
macro_rules! builtin {
    ($dir:literal: $($id:literal),* $(,)?) => {
        &[$(($id, include_str!(concat!("../../../plugins/", $dir, "/", $id, ".css")))),*]
    };
}
impl Kind {
    fn dir(self) -> &'static str {
        match self {
            Kind::Themes => "themes",
            Kind::PublicThemes => "public-notes-themes",
            Kind::PdfPrints => "pdf-prints",
        }
    }
    fn parse(dir: &str) -> Option<Kind> {
        [Kind::Themes, Kind::PublicThemes, Kind::PdfPrints]
            .into_iter()
            .find(|k| k.dir() == dir)
    }
    fn builtin(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Kind::Themes => builtin!("themes": "colorful", "terminal", "classic-light"),
            Kind::PublicThemes => {
                builtin!("public-notes-themes": "midnight", "paper", "sunrise", "terminal")
            }
            Kind::PdfPrints => builtin!("pdf-prints":
                "01-standard", "02-company-report", "03-meeting-minutes", "04-proposal",
                "05-technical-documentation", "06-academic-paper", "07-study-notes",
                "08-executive-brief", "09-research-report", "10-project-status-report",
                "11-knowledge-base-article", "12-client-deliverable", "13-sop-procedure",
                "14-newsletter", "15-book", "16-resume", "17-legal-policy",
                "18-presentation-handout", "19-security-assessment", "20-personal-journal",
            ),
        }
    }
}

/// The PDF template used when none is chosen.
pub(super) const DEFAULT_PDF_TEMPLATE: &str = "01-standard";
/// The page layout used when a PDF template does not provide its own.
pub(super) const DEFAULT_LAYOUT: &str = include_str!("../../../web/pdf-layout.html");
/// Shared styles for every PDF template (regions, cover, tables of contents).
pub(super) const PDF_BASE_CSS: &str = include_str!("../../../web/pdf-base.css");
const BUILTIN_LAYOUTS: &[(&str, &str)] = &[(
    "16-resume",
    include_str!("../../../plugins/pdf-prints/16-resume.html"),
)];
/// A PDF template's layout: its own `{id}.html`, or the default layout.
pub(super) fn layout(id: &str) -> String {
    if !valid_id(id) {
        return DEFAULT_LAYOUT.into();
    }
    read(&folder(Kind::PdfPrints).join(format!("{id}.html")))
        .or_else(|| {
            BUILTIN_LAYOUTS
                .iter()
                .find(|(b, _)| *b == id)
                .map(|(_, html)| html.to_string())
        })
        .unwrap_or_else(|| DEFAULT_LAYOUT.into())
}
/// A PDF template's declared settings (from its header).
pub(super) struct PrintTemplate {
    pub css: String,
    pub variables: Vec<String>,
    pub landscape: bool,
    pub toc: bool,
}
pub(super) fn print_template(id: &str) -> Option<PrintTemplate> {
    let css = get(Kind::PdfPrints, id)?;
    let meta = header(&css);
    Some(PrintTemplate {
        variables: variables(&meta),
        landscape: meta.get("orientation").is_some_and(|o| o == "landscape"),
        toc: meta.get("toc").is_some_and(|t| t == "yes"),
        css,
    })
}
fn variables(meta: &HashMap<String, String>) -> Vec<String> {
    meta.get("variables")
        .map(|v| {
            v.split(',')
                .map(crate::metadata_key)
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Fills a layout. `{{key}}` inserts an escaped value; `{{#key}}…{{/key}}` keeps
/// its content only when the value is set, `{{^key}}…{{/key}}` only when it is
/// not; `{{{key}}}` inserts trusted HTML from `html` (the rendered note, table of
/// contents, ...). Unknown keys are empty.
pub(super) fn fill(
    layout: &str,
    values: &HashMap<String, String>,
    html: &HashMap<&str, String>,
) -> String {
    let set = |k: &str| {
        values.get(k).is_some_and(|v| !v.is_empty()) || html.get(k).is_some_and(|v| !v.is_empty())
    };
    let mut out = String::with_capacity(layout.len() * 2);
    let mut rest = layout;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(inner) = rest.strip_prefix("{{{") {
            let Some(end) = inner.find("}}}") else { break };
            out.push_str(html.get(inner[..end].trim()).map_or("", String::as_str));
            rest = &inner[end + 3..];
            continue;
        }
        let Some(end) = rest.find("}}") else { break };
        let tag = rest[2..end].trim();
        rest = &rest[end + 2..];
        if let Some(name) = tag.strip_prefix('#').or_else(|| tag.strip_prefix('^')) {
            let close = format!("{{{{/{name}}}}}");
            let (inner, after) = match rest.find(&close) {
                Some(i) => (&rest[..i], &rest[i + close.len()..]),
                None => (rest, ""),
            };
            if set(name) == tag.starts_with('#') {
                out.push_str(&fill(inner, values, html));
            }
            rest = after;
        } else if !tag.starts_with('/') {
            out.push_str(&escape(values.get(tag).map_or("", String::as_str)));
        }
    }
    out.push_str(rest);
    out
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Plugin stylesheets larger than this are ignored.
const MAX_SIZE: u64 = 512 * 1024;

pub(super) fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
}
fn folder(kind: Kind) -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("MINERVA_PLUGINS").unwrap_or_else(|_| "plugins".into()))
        .join(kind.dir())
}
fn read(path: &std::path::Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    (meta.is_file() && meta.len() <= MAX_SIZE)
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()
}
/// The stylesheet for a plugin: the plugin folder first, then built-ins.
pub(super) fn get(kind: Kind, id: &str) -> Option<String> {
    if !valid_id(id) {
        return None;
    }
    read(&folder(kind).join(format!("{id}.css"))).or_else(|| {
        kind.builtin()
            .iter()
            .find(|(b, _)| *b == id)
            .map(|(_, css)| css.to_string())
    })
}
/// `key: value` lines from the leading comment.
fn header(css: &str) -> HashMap<String, String> {
    let Some(body) = css
        .trim_start()
        .strip_prefix("/*")
        .and_then(|rest| rest.split_once("*/"))
        .map(|(h, _)| h)
    else {
        return HashMap::new();
    };
    body.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().trim_start_matches('*').trim().split_once(':')?;
            let key = key.trim().to_lowercase();
            (key.chars().all(|c| c.is_ascii_alphabetic()) && !key.is_empty())
                .then(|| (key, value.trim().chars().take(200).collect()))
        })
        .collect()
}
/// Every available plugin of a kind, sorted by name.
pub(super) fn list(kind: Kind) -> Vec<Value> {
    let mut found: HashMap<String, String> = kind
        .builtin()
        .iter()
        .map(|(id, css)| (id.to_string(), css.to_string()))
        .collect();
    if let Ok(entries) = std::fs::read_dir(folder(kind)) {
        for entry in entries.flatten().take(200) {
            let path = entry.path();
            let Some(id) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".css"))
                .filter(|id| valid_id(id))
            else {
                continue;
            };
            if let Some(css) = read(&path) {
                found.insert(id.to_string(), css);
            }
        }
    }
    let mut themes: Vec<Value> = found
        .into_iter()
        .map(|(id, css)| {
            let meta = header(&css);
            let field = |k: &str| meta.get(k).cloned().unwrap_or_default();
            let mut v = json!({"id": id, "name": meta.get("name").cloned().unwrap_or_else(|| id.clone()), "description": field("description")});
            match kind {
                // App themes extend light or dark; anything else is treated as light.
                Kind::Themes => v["base"] = json!(if field("base") == "dark" { "dark" } else { "light" }),
                Kind::PublicThemes => {
                    v["scheme"] = json!(match field("scheme").as_str() {
                        s @ ("light" | "dark") => s,
                        _ => "",
                    })
                }
                Kind::PdfPrints => {
                    v["users"] = json!(field("users"));
                    v["variables"] = json!(variables(&meta));
                    v["orientation"] = json!(if field("orientation") == "landscape" { "landscape" } else { "portrait" });
                }
            }
            v
        })
        .collect();
    // PDF templates keep their numbered order; themes sort by name.
    if kind == Kind::PdfPrints {
        themes.sort_by_key(|t| t["id"].as_str().unwrap_or("").to_string());
    } else {
        themes.sort_by_key(|t| t["name"].as_str().unwrap_or("").to_lowercase());
    }
    themes
}
/// The `scheme` (`light`, `dark`, or empty) a public theme declares.
pub(super) fn scheme(id: &str) -> String {
    get(Kind::PublicThemes, id)
        .map(|css| header(&css).remove("scheme").unwrap_or_default())
        .filter(|s| s == "light" || s == "dark")
        .unwrap_or_default()
}

/// `GET /plugins/index.json` and `GET /plugins/{kind}/{id}.css`.
pub(super) async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches("/plugins/");
    if path == "index.json" {
        let index = json!({"themes": list(Kind::Themes), "public_themes": list(Kind::PublicThemes), "pdf_prints": list(Kind::PdfPrints)});
        return ([("cache-control", "no-cache")], axum::Json(index)).into_response();
    }
    let css = path
        .split_once('/')
        .and_then(|(dir, file)| get(Kind::parse(dir)?, file.strip_suffix(".css")?));
    match css {
        Some(css) => (
            [
                ("content-type", "text/css; charset=utf-8"),
                ("cache-control", "no-cache"),
            ],
            css,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers_and_ids() {
        let meta = header("/*\n * name: Paper\n * scheme: light\n */\n:root{}");
        assert_eq!(meta["name"], "Paper");
        assert_eq!(meta["scheme"], "light");
        assert!(header(":root{}").is_empty());
        assert!(valid_id("my-theme2"));
        for bad in ["", "../x", "A", "a.b", "-x"] {
            assert!(!valid_id(bad), "{bad}");
        }
        for kind in [Kind::Themes, Kind::PublicThemes, Kind::PdfPrints] {
            for (id, css) in kind.builtin() {
                assert!(valid_id(id));
                assert!(header(css).contains_key("name"), "{id} needs a name");
            }
        }
    }
}
