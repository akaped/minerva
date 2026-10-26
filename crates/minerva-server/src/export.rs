//! Note export: Markdown, Markdown with attachments (.zip), a standalone HTML
//! file, and a print-designed page that the browser saves as PDF.
use super::*;
use crate::render::{render_for, Links};
use axum::extract::{Path, Query};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::Write;

/// Total bytes of images embedded as data URIs in one HTML export.
const EMBED_LIMIT: usize = 50 * 1024 * 1024;

pub(super) const EXPORT_CSS: &str = include_str!("../../../web/export.css");
pub(super) const EXPORT_JS: &str = include_str!("../../../web/export.js");

fn flag(q: &HashMap<String, String>, key: &str, default: bool) -> bool {
    q.get(key).map_or(default, |v| v == "1" || v == "true")
}

/// `/export/{note_id}?format=md|zip|html|print`.
pub(super) async fn export(
    State(store): State<Shared>,
    Path(nid): Path<String>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    tokio::task::spawn_blocking(move || {
        let s = store.lock().map_err(|_| internal())?;
        let u = authenticate(&s.data, &headers)?;
        let n = s.data.notes.iter().find(|n| n.id == nid).ok_or_else(missing)?;
        note_access(&s.data, &u, n, false)?;
        let name = file_stem(&n.title);
        let options = Options {
            front_matter: flag(&q, "front_matter", true),
            cover: flag(&q, "cover", true),
            meta: flag(&q, "meta", true),
            letter: q.get("size").is_some_and(|s| s == "letter"),
            // Without a valid choice, documents use the Standard template.
            template: q
                .get("template")
                .map(String::as_str)
                .into_iter()
                .chain([plugins::DEFAULT_PDF_TEMPLATE])
                .find(|t| plugins::get(plugins::Kind::PdfPrints, t).is_some())
                .map(String::from),
        };
        match q.get("format").map(String::as_str).unwrap_or("print") {
            "md" => {
                let site = sharing::site_url(&headers);
                let text = markdown(&s.data, &u, n, &options, &|id, image| {
                    format!("{site}/api/v1/attachments/{id}{}", if image { "/preview" } else { "" })
                });
                Ok(download(text.into_bytes(), "text/markdown; charset=utf-8", &format!("{name}.md")))
            }
            "zip" => Ok(download(zip(&s, &u, n, &options)?, "application/zip", &format!("{name}.zip"))),
            "html" => {
                let html = document(&s, &u, n, &options, true, &sharing::site_url(&headers));
                Ok(download(html.into_bytes(), "text/html; charset=utf-8", &format!("{name}.html")))
            }
            "print" => {
                let mut response = axum::response::Html(document(&s, &u, n, &options, false, "")).into_response();
                // This page is printed from a hidden frame inside the app.
                response.headers_mut().insert(
                    "content-security-policy",
                    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: http: https:; media-src 'self' http: https:; frame-ancestors 'self'; base-uri 'none'; form-action 'none'"
                        .parse()
                        .unwrap(),
                );
                Ok(response)
            }
            _ => Err(bad("Choose md, zip, html or print")),
        }
    })
    .await
    .map_err(|_| internal())?
}

struct Options {
    front_matter: bool,
    cover: bool,
    meta: bool,
    letter: bool,
    /// A PDF template from `plugins/pdf-prints`; `None` keeps the plain document.
    template: Option<String>,
}

/// A safe file name stem from a note title.
fn file_stem(title: &str) -> String {
    let stem: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = stem
        .trim()
        .trim_matches('.')
        .chars()
        .take(100)
        .collect::<String>();
    if stem.is_empty() {
        "note".into()
    } else {
        stem
    }
}
fn download(bytes: Vec<u8>, media: &str, filename: &str) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(filename.as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    let mut response = bytes.into_response();
    let h = response.headers_mut();
    h.insert("content-type", media.parse().unwrap());
    h.insert(
        "content-disposition",
        format!("attachment; filename=\"export\"; filename*=UTF-8''{encoded}")
            .parse()
            .unwrap(),
    );
    response
}

/// Rewrites Minerva-specific destinations in block Markdown: attachment URLs via
/// `attachment(id, image)` and note links `[label](#note/{id})` to `[[Title]]`.
fn portable(d: &Data, markdown: &str, attachment: &dyn Fn(&str, bool) -> String) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(at) = rest.find("](") {
        let (before, after) = rest.split_at(at);
        let destination = &after[2..];
        if let Some(id) = destination.strip_prefix("#note/").and_then(|d| d.get(..36)) {
            if destination[42..].starts_with(')') && Uuid::parse_str(id).is_ok() {
                // Find the label's opening bracket (labels escape inner brackets).
                let bytes = before.as_bytes();
                let open = (0..bytes.len())
                    .rev()
                    .find(|&i| bytes[i] == b'[' && (i == 0 || bytes[i - 1] != b'\\'));
                if let Some(open) = open {
                    let label = before[open + 1..].replace("\\[", "[").replace("\\]", "]");
                    let title = d
                        .notes
                        .iter()
                        .find(|n| n.id == id)
                        .map_or(label, |n| n.title.clone());
                    out.push_str(&before[..open]);
                    out.push_str(&format!("[[{title}]]"));
                    rest = &destination[43..];
                    continue;
                }
            }
        }
        if let Some(tail) = destination.strip_prefix("/api/v1/attachments/") {
            if let Some(id) = tail.get(..36).filter(|id| Uuid::parse_str(id).is_ok()) {
                let after_id = &tail[36..];
                let (image, after_id) = match after_id.strip_prefix("/preview") {
                    Some(r) => (true, r),
                    None => (false, after_id),
                };
                if after_id.starts_with(')') {
                    out.push_str(before);
                    out.push_str("](");
                    out.push_str(&attachment(id, image));
                    rest = after_id;
                    continue;
                }
            }
        }
        out.push_str(before);
        out.push_str("](");
        rest = destination;
    }
    out.push_str(rest);
    out
}

fn is_list_item(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") || {
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") "))
    }
}

/// Embedded databases become a snapshot table of their current records when the
/// exporting member can read them.
fn with_databases<'a>(d: &Data, u: &User, markdown: &'a str) -> std::borrow::Cow<'a, str> {
    if !markdown.contains(render::DATABASE_EMBED) {
        return markdown.into();
    }
    outside_code(markdown, |text| expand_databases(d, u, text).into_owned()).into()
}
/// Replaces `{{key}}` with the note's metadata (and built-in values such as
/// `{{date}}`) in exports. Unknown keys stay as written; code stays literal.
fn with_values<'a>(
    markdown: &'a str,
    values: &HashMap<String, String>,
) -> std::borrow::Cow<'a, str> {
    if !markdown.contains("{{") {
        return markdown.into();
    }
    outside_code(markdown, |text| {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find("}}").map(|end| (after[..end].trim(), end)) {
                Some((key, end)) if values.contains_key(key) => {
                    out.push_str(&values[key]);
                    rest = &after[end + 2..];
                }
                _ => {
                    out.push_str("{{");
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    })
    .into()
}
/// Applies `f` to the parts of `markdown` outside fenced code and backtick spans.
fn outside_code(markdown: &str, f: impl Fn(&str) -> String) -> String {
    let mut fenced = false;
    markdown
        .split('\n')
        .map(|line| {
            let fence =
                line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~");
            if fence || fenced {
                fenced ^= fence;
                return line.to_string();
            }
            line.split('`')
                .enumerate()
                .map(|(i, part)| {
                    if i % 2 == 1 {
                        part.to_string()
                    } else {
                        f(part)
                    }
                })
                .collect::<Vec<_>>()
                .join("`")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
/// Values for `{{key}}` placeholders and PDF templates: the note's metadata, plus
/// `title`, `date` (today), `generated`, `updated`, and `tags` unless set.
fn values(n: &Note) -> HashMap<String, String> {
    let mut values: HashMap<String, String> = n.metadata.clone().into_iter().collect();
    let now = now();
    let defaults = [
        ("title", n.title.clone()),
        // A journal entry's own date, otherwise today.
        (
            "date",
            n.journal.clone().unwrap_or_else(|| sharing::date(now)),
        ),
        (
            "generated",
            format!(
                "{} {} UTC",
                sharing::date(now),
                &sharing::rfc3339(now)[11..16]
            ),
        ),
        ("updated", sharing::date(n.updated_at)),
        ("tags", n.tags.join(", ")),
    ];
    for (key, value) in defaults {
        values.entry(key.into()).or_insert(value);
    }
    // Images (a logo or photo) may be an attachment, an HTTPS image, or nothing,
    // which prints a placeholder.
    for key in ["logo", "photo"] {
        let Some(image) = values.get(key).cloned() else {
            continue;
        };
        match render::attachment(&image) {
            Some((id, false)) => {
                values.insert(key.into(), format!("/api/v1/attachments/{id}/preview"));
            }
            _ if image.starts_with("https://") => {}
            _ => {
                values.remove(key);
            }
        }
    }
    values
}
fn label(key: &str) -> String {
    match key {
        "client" => "Prepared for".into(),
        "prepared_for" => "Prepared for".into(),
        "document_id" => "Document ID".into(),
        "approval_status" => "Approval status".into(),
        "url" => "URL".into(),
        _ => {
            let words = key.replace('_', " ");
            let mut chars = words.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        }
    }
}
/// The template's variables in its order, then any other metadata of the note.
fn ordered_keys(n: &Note, template: &plugins::PrintTemplate) -> Vec<String> {
    let mut keys = template.variables.clone();
    for key in n.metadata.keys() {
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    keys
}
fn fact_value(values: &HashMap<String, String>, key: &str) -> String {
    match values.get(key).filter(|v| !v.is_empty()) {
        Some(v) => escape(v),
        // The variable to set, so the reader learns its name from the PDF.
        None => format!(
            "<span class=\"pdf-placeholder\">&#123;&#123;{}&#125;&#125;</span>",
            escape(key)
        ),
    }
}
/// Adds IDs to headings and lists them, for the table of contents.
fn number_headings(html: &str) -> (String, String) {
    let mut out = String::with_capacity(html.len());
    let mut toc = String::new();
    let mut rest = html;
    let mut count = 0;
    while let Some((start, level)) = ["<h1>", "<h2>", "<h3>"]
        .iter()
        .enumerate()
        .filter_map(|(i, tag)| rest.find(tag).map(|at| (at, i + 1)))
        .min()
    {
        count += 1;
        out.push_str(&rest[..start]);
        out.push_str(&format!("<h{level} id=\"sec-{count}\">"));
        rest = &rest[start + 4..];
        let close = format!("</h{level}>");
        let end = rest.find(&close).unwrap_or(rest.len());
        // Inline #tags are labels, not part of the heading's name.
        let mut inner = rest[..end].to_string();
        while let Some(at) = inner.find("<span class=\"tag\"") {
            let close = inner[at..]
                .find("</span>")
                .map_or(inner.len(), |i| at + i + 7);
            inner.replace_range(at..close, "");
        }
        let mut text = String::new();
        let mut in_tag = false;
        for c in inner.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                c if !in_tag => text.push(c),
                _ => {}
            }
        }
        toc.push_str(&format!(
            "<li class=\"toc-{level}\"><a href=\"#sec-{count}\">{}</a></li>",
            text.trim()
        ));
    }
    out.push_str(rest);
    let toc = if toc.is_empty() {
        String::new()
    } else {
        format!("<h2>Contents</h2><ol>{toc}</ol>")
    };
    (out, toc)
}
fn expand_databases<'a>(d: &Data, u: &User, markdown: &'a str) -> std::borrow::Cow<'a, str> {
    render::replace_database_embeds(markdown, |body| {
        let id = render::database_embed(body).map_or("", |(id, _)| id);
        match d.databases.iter().find(|db| db.id == id) {
            Some(db) if access(d, u, &db.folder_id, false).is_ok() => {
                format!("\n\n{}\n", databases::markdown_table(db))
            }
            Some(_) => "*Embedded database (not available to you)*".into(),
            None => "*Embedded database (deleted)*".into(),
        }
    })
}
fn markdown(
    d: &Data,
    u: &User,
    n: &Note,
    options: &Options,
    attachment: &dyn Fn(&str, bool) -> String,
) -> String {
    let mut text = String::new();
    if options.front_matter {
        // JSON strings and arrays are valid YAML flow scalars.
        text.push_str("---\n");
        text.push_str(&format!("title: {}\n", json!(n.title)));
        if !n.tags.is_empty() {
            text.push_str(&format!("tags: {}\n", json!(n.tags)));
        }
        if !n.icon.is_empty() {
            text.push_str(&format!("icon: {}\n", json!(n.icon)));
        }
        if !n.cover.is_empty() {
            let cover = match render::attachment(&n.cover) {
                Some((id, false)) => attachment(&id.to_string(), true),
                _ => n.cover.clone(),
            };
            text.push_str(&format!("cover: {}\n", json!(cover)));
        }
        text.push_str(&format!("updated: {}\n", sharing::rfc3339(n.updated_at)));
        for (key, value) in &n.metadata {
            if !matches!(
                key.as_str(),
                "title" | "tags" | "icon" | "cover" | "updated"
            ) {
                text.push_str(&format!("{key}: {}\n", json!(value)));
            }
        }
        text.push_str("---\n\n");
    }
    text.push_str(&format!("# {}", n.title));
    let mut previous: Option<&str> = None;
    let values = values(n);
    for block in &n.blocks {
        let body = portable(
            d,
            &with_databases(d, u, &with_values(&block.markdown, &values)),
            attachment,
        );
        // Consecutive list items stay one tight list; other blocks are paragraphs.
        let tight = previous.is_some_and(|p| is_list_item(p) && is_list_item(&block.markdown));
        text.push_str(if tight { "\n" } else { "\n\n" });
        text.push_str(&body);
        previous = Some(&block.markdown);
    }
    text.push('\n');
    text
}

/// An attachment included in an export.
struct File {
    id: String,
    filename: String,
    media: String,
    bytes: Vec<u8>,
}
/// Attachments referenced by a note (blocks and cover) that the user may read.
fn referenced_files(s: &Store, u: &User, n: &Note) -> Result<Vec<File>, ApiError> {
    let mut ids: Vec<String> = vec![];
    for text in n
        .blocks
        .iter()
        .map(|b| b.markdown.as_str())
        .chain(std::iter::once(n.cover.as_str()))
    {
        let mut rest = text;
        while let Some(at) = rest.find("/api/v1/attachments/") {
            let tail = &rest[at + 20..];
            if let Some(id) = tail.get(..36).filter(|id| Uuid::parse_str(id).is_ok()) {
                if !ids.iter().any(|known| known == id) {
                    ids.push(id.to_string());
                }
            }
            rest = tail;
        }
    }
    let mut files = vec![];
    for id in ids {
        let row: Option<(String, String, String, Vec<u8>)> =
            s.db.query_row(
                "SELECT note_id,filename,media_type,content FROM attachments WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|_| internal())?;
        if let Some((owner, filename, media, bytes)) = row {
            let readable = s
                .data
                .notes
                .iter()
                .find(|o| o.id == owner)
                .is_some_and(|o| note_access(&s.data, u, o, false).is_ok());
            if readable {
                files.push(File {
                    id,
                    filename,
                    media,
                    bytes,
                });
            }
        }
    }
    Ok(files)
}

fn zip(s: &Store, u: &User, n: &Note, options: &Options) -> Result<Vec<u8>, ApiError> {
    let files = referenced_files(s, u, n)?;
    // Unique names inside files/, keeping extensions.
    let mut paths: HashMap<String, String> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    for File { id, filename, .. } in &files {
        let (stem, ext) = filename
            .rsplit_once('.')
            .map_or((filename.as_str(), String::new()), |(s, e)| {
                (s, format!(".{e}"))
            });
        let mut candidate = format!("files/{filename}");
        let mut counter = 1;
        while !used.insert(candidate.to_lowercase()) {
            candidate = format!("files/{stem} ({counter}){ext}");
            counter += 1;
        }
        paths.insert(id.clone(), candidate);
    }
    let text = markdown(&s.data, u, n, options, &|id, _| {
        paths
            .get(id)
            .map(|p| p.replace(' ', "%20"))
            .unwrap_or_else(|| format!("/api/v1/attachments/{id}"))
    });
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive
        .start_file(format!("{}.md", file_stem(&n.title)), stored)
        .map_err(|_| internal())?;
    archive.write_all(text.as_bytes()).map_err(|_| internal())?;
    for File { id, bytes, .. } in &files {
        archive
            .start_file(paths[id].clone(), stored)
            .map_err(|_| internal())?;
        archive.write_all(bytes).map_err(|_| internal())?;
    }
    Ok(archive.finish().map_err(|_| internal())?.into_inner())
}

/// The exported document. `standalone` inlines styles and embeds images so the
/// file works offline; otherwise it links to the server (print page).
fn document(
    s: &Store,
    u: &User,
    n: &Note,
    options: &Options,
    standalone: bool,
    site: &str,
) -> String {
    let values = values(n);
    let template = options
        .template
        .as_deref()
        .and_then(plugins::print_template);
    let mut body: String = n
        .blocks
        .iter()
        .map(|b| {
            format!(
                "<div class=\"block\">{}</div>\n",
                render_for(
                    &with_databases(&s.data, u, &with_values(&b.markdown, &values)),
                    Links::Export
                )
            )
        })
        .collect();
    let mut toc = String::new();
    if template.is_some() {
        (body, toc) = number_headings(&body);
    }
    let mut cover = String::new();
    if options.cover && !n.cover.is_empty() {
        cover = if let Some(gradient) = n.cover.strip_prefix("gradient:") {
            format!(
                "<div class=\"export-cover cover-gradient-{}\"></div>",
                escape(gradient)
            )
        } else {
            let src = match render::attachment(&n.cover) {
                Some((id, false)) => format!("/api/v1/attachments/{id}/preview"),
                _ => n.cover.clone(),
            };
            format!(
                "<div class=\"export-cover\"><img src=\"{}\" alt=\"\"></div>",
                escape(&src)
            )
        };
    }
    let tags: Vec<String> = n
        .tags
        .iter()
        .chain(inline_tags(n).iter())
        .cloned()
        .collect::<Vec<_>>();
    let mut unique_tags: Vec<String> = vec![];
    for tag in tags {
        if !unique_tags.contains(&tag) {
            unique_tags.push(tag);
        }
    }
    let meta = if options.meta {
        format!(
            "<p class=\"export-meta\"><time datetime=\"{}\">Updated {}</time>{}</p>",
            sharing::rfc3339(n.updated_at),
            sharing::date(n.updated_at),
            unique_tags
                .iter()
                .map(|t| format!(" <span class=\"tag\">#{}</span>", escape(t)))
                .collect::<String>()
        )
    } else {
        String::new()
    };
    let icon = if n.icon.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"post-icon\" aria-hidden=\"true\">{}</div>",
            escape(&n.icon)
        )
    };
    let title = escape(&n.title);
    let (mut page, extra_css, body_class, orientation) = match (&template, &options.template) {
        (Some(t), Some(id)) => {
            let keys = ordered_keys(n, t);
            let shown = |k: &&String| {
                !matches!(
                    k.as_str(),
                    "title" | "subtitle" | "logo" | "abstract" | "keywords" | "doc_type"
                )
            };
            // The cover shows the template's own fields; other metadata goes in
            // the document information table.
            let facts: String = t
                .variables
                .iter()
                .filter(shown)
                .filter(|k| !matches!(k.as_str(), "classification"))
                .map(|k| {
                    format!(
                        "<div><dt>{}</dt><dd>{}</dd></div>",
                        escape(&label(k)),
                        fact_value(&values, k)
                    )
                })
                .collect();
            let table: String = keys
                .iter()
                .filter(shown)
                .map(|k| {
                    format!(
                        "<tr><th>{}</th><td>{}</td></tr>",
                        escape(&label(k)),
                        fact_value(&values, k)
                    )
                })
                .collect();
            let regions = HashMap::from([
                ("content", body.clone()),
                ("toc", if t.toc { toc.clone() } else { String::new() }),
                ("titleblock", format!("{icon}<h1>{title}</h1>\n{meta}")),
                (
                    "cover_image",
                    cover.replace(
                        "class=\"export-cover",
                        "class=\"export-cover pdf-cover-image",
                    ),
                ),
                (
                    "cover_facts",
                    if facts.is_empty() {
                        String::new()
                    } else {
                        format!("<dl class=\"pdf-facts\">{facts}</dl>")
                    },
                ),
                (
                    "metadata_table",
                    if table.is_empty() {
                        String::new()
                    } else {
                        format!("<table>{table}</table>")
                    },
                ),
            ]);
            (
                plugins::fill(&plugins::layout(id), &values, &regions),
                format!(
                    "<style>{}</style><style>{}</style>",
                    plugins::PDF_BASE_CSS,
                    t.css
                ),
                format!("export pdf pdf-{id}"),
                if t.landscape { " landscape" } else { "" },
            )
        }
        _ => (
            format!("{cover}<article>\n{icon}<h1>{title}</h1>\n{meta}\n{body}</article>"),
            String::new(),
            "export".into(),
            "",
        ),
    };
    if standalone {
        // Embed this note's readable images; link everything else to the server.
        let mut budget = EMBED_LIMIT;
        for File {
            id, media, bytes, ..
        } in referenced_files(s, u, n).unwrap_or_default()
        {
            let preview = format!("/api/v1/attachments/{id}/preview");
            if media.starts_with("image/") && bytes.len() <= budget {
                budget -= bytes.len();
                let data = format!("data:{media};base64,{}", STANDARD.encode(&bytes));
                page = page.replace(&format!("src=\"{preview}\""), &format!("src=\"{data}\""));
            }
        }
        page = page.replace("=\"/api/v1/", &format!("=\"{site}/api/v1/"));
    }
    // Styles are embedded so the document never depends on loading order or on
    // the reader's theme.
    let styles = format!(
        "<style>{EXPORT_CSS}</style>{extra_css}{}",
        if standalone {
            ""
        } else {
            "<script src=\"/export.js\" defer></script>"
        }
    );
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
{styles}
<style>@page {{ size: {size}{orientation}; }}</style>
</head>
<body class="{body_class}">
<main class="post export-document">
{page}
</main>
</body>
</html>"#,
        size = if options.letter { "letter" } else { "A4" },
    )
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rewrites_note_links_and_attachments_portably() {
        let mut d = Data::default();
        let target = Uuid::new_v4().to_string();
        d.notes.push(Note {
            id: target.clone(),
            folder_id: "f".into(),
            title: "Renamed target".into(),
            icon: String::new(),
            blocks: vec![],
            revision: 1,
            updated_at: 0,
            favorite: false,
            trashed: false,
            cover: String::new(),
            tags: vec![],
            journal: None,
            metadata: Default::default(),
            print_template: String::new(),
        });
        let file = Uuid::new_v4();
        let md = format!("See [Old \\[name\\]](#note/{target}) and ![chart](/api/v1/attachments/{file}/preview) or [file](/api/v1/attachments/{file}) and [web](https://example.com).");
        let out = portable(&d, &md, &|id, image| {
            format!("files/{id}{}", if image { "-img" } else { "" })
        });
        assert_eq!(out, format!("See [[Renamed target]] and ![chart](files/{file}-img) or [file](files/{file}) and [web](https://example.com)."));
        assert_eq!(file_stem("a/b: c?"), "a_b_ c_");
        assert_eq!(file_stem("..."), "note");
        assert!(is_list_item("- [ ] task") && is_list_item("12. item") && !is_list_item("-not"));
    }
}
