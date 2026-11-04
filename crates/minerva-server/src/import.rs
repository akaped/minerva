//! Imports from other apps into a folder (`POST /api/v1/import?folder_id=…&name=…`
//! with the file as the body): a Notion export (“Markdown & CSV”, including the
//! zips Notion nests in it), an Obsidian vault or a Logseq graph as a zip of its
//! folder, an Evernote export (`.enex`), or one Markdown file.
//!
//! Every page becomes a note, in subfolders like the export's folders when the
//! person can edit the workspace, otherwise all in the chosen folder. Front matter
//! and Logseq page properties become tags and note details; `[[links]]`, relative
//! links and Logseq block references point at the imported notes; the files a page
//! uses become its attachments; Notion's CSV tables become databases. Markdown is
//! split into blocks as if typed: a block per paragraph and per list item, nested
//! by indentation.
use super::*;
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

/// The largest file accepted, and what an archive may unpack to.
pub(super) const IMPORT_LIMIT: usize = 200 * 1024 * 1024;
const UNPACKED_LIMIT: usize = 1024 * 1024 * 1024;
const ENTRIES: usize = 20_000;
const PAGES: usize = 5_000;
const BLOCKS: usize = 2_000;
const BLOCK_BYTES: usize = 100_000;

struct Page {
    dir: Vec<String>,
    /// Where the page was in the export (for relative links); empty for Evernote.
    path: String,
    title: String,
    markdown: String,
    tags: Vec<String>,
    metadata: BTreeMap<String, String>,
}
struct Table {
    dir: Vec<String>,
    title: String,
    rows: Vec<Vec<String>>,
}
#[derive(Default)]
struct Export {
    pages: Vec<Page>,
    tables: Vec<Table>,
    /// Files by path in the export; Evernote's by `evernote:<md5>`.
    files: HashMap<String, Vec<u8>>,
    /// Evernote file names, by the same key.
    names: HashMap<String, String>,
    warnings: Vec<String>,
}

/// `POST /api/v1/import?folder_id=…&name=…`.
pub(super) async fn endpoint(
    State(store): State<Shared>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    if headers
        .get("x-minerva-client")
        .and_then(|h| h.to_str().ok())
        != Some("1")
    {
        return Err(bad("X-Minerva-Client: 1 is required"));
    }
    let query: HashMap<String, String> =
        url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    let folder = query.get("folder_id").cloned().unwrap_or_default();
    let name = query.get("name").cloned().unwrap_or_default();
    // Check access before the work of reading the file.
    {
        let s = store.lock().map_err(|_| internal())?;
        let u = authenticate(&s.data, &headers)?;
        access(&s.data, &u, &folder, true)?;
    }
    tokio::task::spawn_blocking(move || {
        let export = read(&name, &body)?;
        let mut s = store.lock().map_err(|_| internal())?;
        let u = authenticate(&s.data, &headers)?;
        access(&s.data, &u, &folder, true)?;
        let report = apply(&mut s, &u, &folder, export)?;
        Ok(axum::Json(report).into_response())
    })
    .await
    .map_err(|_| internal())?
}

/// Reads an export: a zip, an Evernote file, or a Markdown file.
fn read(name: &str, bytes: &[u8]) -> Result<Export, ApiError> {
    if bytes.is_empty() {
        return Err(bad("The file is empty"));
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let mut entries = vec![];
        let mut total = 0;
        unzip(bytes, &mut entries, 0, &mut total)?;
        return Ok(from_files(entries));
    }
    let text = String::from_utf8_lossy(bytes);
    if name.to_lowercase().ends_with(".enex") || text.contains("<en-export") {
        return evernote(&text);
    }
    let lower = name.to_lowercase();
    if lower.ends_with(".mht") || lower.ends_with(".mhtml") {
        return mhtml(name, bytes);
    }
    let mut export = Export::default();
    let stem = name.rsplit('/').next().unwrap_or(name);
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    export.pages.push(page(vec![], name, stem, &text));
    Ok(export)
}

fn unzip(
    bytes: &[u8],
    out: &mut Vec<(String, Vec<u8>)>,
    depth: u8,
    total: &mut usize,
) -> Result<(), ApiError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| bad("This zip file cannot be read"))?;
    if archive.len() > ENTRIES {
        return Err(bad("The archive has more than 20,000 files"));
    }
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| bad("This zip file cannot be read"))?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let path = path.to_string_lossy().replace('\\', "/");
        let mut data = vec![];
        (&mut entry)
            .take(attachments::FILE_LIMIT as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|_| bad("This zip file cannot be read"))?;
        *total += data.len();
        if *total > UNPACKED_LIMIT {
            return Err(bad("The archive unpacks to more than 1 GB"));
        }
        // Notion splits large exports into zips inside the zip; a Bear
        // `.textpack` is a zipped TextBundle.
        let lower = path.to_lowercase();
        if (lower.ends_with(".zip") || lower.ends_with(".textpack")) && depth == 0 {
            unzip(&data, out, 1, total)?;
        } else {
            out.push((path, data));
        }
    }
    Ok(())
}

/// Folders and files apps keep for themselves.
fn ignored(path: &str) -> bool {
    path.split('/').any(|part| {
        part.starts_with('.') || part == "__MACOSX" || part == "logseq" || part == "_resources"
    })
}
/// A name without the ID Notion appends (` 1a2b…` of 32 hexadecimal digits).
fn without_notion_id(name: &str) -> &str {
    match name.rsplit_once(' ') {
        Some((base, id)) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => base,
        _ => name,
    }
}
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_files(entries: Vec<(String, Vec<u8>)>) -> Export {
    let mut entries: Vec<_> = entries.into_iter().filter(|(p, _)| !ignored(p)).collect();
    // One folder around everything (a zipped vault) is not a folder of its own.
    let root = entries
        .first()
        .and_then(|(p, _)| p.split_once('/'))
        .map(|(r, _)| format!("{r}/"));
    // A TextBundle is a note, not a folder around the export.
    if let Some(root) = root.filter(|r| !r.to_lowercase().ends_with(".textbundle/")) {
        if entries.iter().all(|(p, _)| p.starts_with(&root)) {
            for (p, _) in &mut entries {
                *p = p[root.len()..].to_string();
            }
        }
    }
    let paths: HashSet<String> = entries.iter().map(|(p, _)| p.to_lowercase()).collect();
    let mut export = Export::default();
    for (path, data) in entries {
        let (dir, file) = path
            .rsplit_once('/')
            .map_or(("", path.as_str()), |(d, f)| (d, f));
        let dir: Vec<String> = dir
            .split('/')
            .filter(|d| !d.is_empty())
            .map(|d| without_notion_id(d).to_string())
            .collect();
        let (stem, ext) = file.rsplit_once('.').unwrap_or((file, ""));
        match ext.to_lowercase().as_str() {
            "md" | "markdown" => {
                if export.pages.len() >= PAGES {
                    export
                        .warnings
                        .push("Only the first 5,000 pages were imported".into());
                    continue;
                }
                let text = String::from_utf8_lossy(&data);
                // A TextBundle (Bear): `Title.textbundle/text.md`.
                let bundle = dir
                    .last()
                    .and_then(|d| d.strip_suffix(".textbundle"))
                    .map(String::from);
                match bundle {
                    Some(title) if stem == "text" => {
                        let outer = dir[..dir.len() - 1].to_vec();
                        export.pages.push(page(outer, &path, &title, &text));
                    }
                    _ => export.pages.push(page(dir, &path, stem, &text)),
                }
            }
            "csv" => {
                // Notion writes a table twice; the `_all` file has every row.
                let base = stem.strip_suffix("_all");
                let twin = format!("{}{}.csv", &path[..path.len() - file.len()], stem);
                if base.is_none()
                    && paths.contains(&twin.replace(".csv", "_all.csv").to_lowercase())
                {
                    continue;
                }
                let text = String::from_utf8_lossy(&data);
                let rows = anki::csv(text.trim_start_matches('\u{feff}'), ',');
                if rows.len() > 1 {
                    export.tables.push(Table {
                        dir,
                        title: without_notion_id(base.unwrap_or(stem)).to_string(),
                        rows,
                    });
                }
            }
            _ => {
                export.files.insert(path, data);
            }
        }
    }
    export
}

/// A page from Markdown: its title, tags and details from front matter or
/// Logseq properties, and its text.
fn page(dir: Vec<String>, path: &str, stem: &str, text: &str) -> Page {
    let text = text
        .replace("\r\n", "\n")
        .trim_start_matches('\u{feff}')
        .to_string();
    let mut title = without_notion_id(stem).to_string();
    // Logseq names namespaced pages `a___b` (or `a%2Fb`), and journals `2026_11_01`.
    if dir.last().is_some_and(|d| d == "journals")
        && stem.len() == 10
        && stem.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'_'
            } else {
                b.is_ascii_digit()
            }
        })
    {
        title = stem.replace('_', "-");
    } else {
        title = percent_decode(&title.replace("___", "/"));
    }
    let mut properties: Vec<(String, String)> = vec![];
    let mut body = text.as_str();
    if let Some(rest) = body.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            properties.extend(front_matter(&rest[..end]));
            body = rest[end + 4..]
                .trim_start_matches(|c| c != '\n')
                .trim_start_matches('\n');
        }
    }
    // Logseq page properties: `key:: value` lines before the first block.
    let mut lines = body.lines().peekable();
    let mut skip = 0;
    while let Some(line) = lines.peek() {
        match line.split_once(":: ") {
            Some((key, value))
                if !key.contains(' ') && !key.starts_with('-') && !key.is_empty() =>
            {
                properties.push((key.to_string(), value.trim().to_string()));
                skip += line.len() + 1;
                lines.next();
            }
            _ => break,
        }
    }
    let mut body = bear_tags(body[skip.min(body.len())..].trim_start_matches('\n'));
    // A first heading repeating the title is the title.
    if let Some(first) = body.lines().next() {
        if let Some(heading) = first.strip_prefix("# ") {
            if heading.trim().eq_ignore_ascii_case(title.trim()) {
                body = body[first.len()..].trim_start_matches('\n').to_string();
            }
        }
    }
    let mut tags = vec![];
    let mut metadata = BTreeMap::new();
    for (key, value) in properties {
        match key.to_lowercase().as_str() {
            "tags" | "tag" => tags.extend(value.split(',').map(|t| {
                t.trim()
                    .trim_start_matches("[[")
                    .trim_end_matches("]]")
                    .to_string()
            })),
            "title" if !value.is_empty() => title = value,
            _ => {
                let key = metadata_key(&key);
                if key.starts_with(|c: char| c.is_ascii_lowercase())
                    && !value.is_empty()
                    && value.chars().count() <= 4000
                    && metadata.len() < 50
                {
                    metadata.insert(key, value);
                }
            }
        }
    }
    let mut normalized: Vec<String> = vec![];
    for tag in tags
        .iter()
        .filter_map(|t| render::normalize_tag(&t.replace(' ', "-")))
    {
        if !normalized.contains(&tag) && normalized.len() < 20 {
            normalized.push(tag);
        }
    }
    Page {
        dir,
        path: path.to_string(),
        title: title.chars().take(200).collect(),
        markdown: body,
        tags: normalized,
        metadata,
    }
}

/// Simple YAML front matter: `key: value`, `key: [a, b]`, and lists of `- item`.
fn front_matter(yaml: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in yaml.lines() {
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if let Some((_, value)) = out.last_mut() {
                if !value.is_empty() {
                    value.push_str(", ");
                }
                value.push_str(item.trim().trim_matches(['"', '\'']));
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            if line.starts_with(' ') {
                continue;
            }
            let value = value.trim().trim_start_matches('[').trim_end_matches(']');
            let value = value
                .split(',')
                .map(|v| v.trim().trim_matches(['"', '\'']))
                .collect::<Vec<_>>()
                .join(", ");
            out.push((key.trim().to_string(), value));
        }
    }
    out
}

/// Markdown as blocks, as if typed: a block per paragraph line and per list item
/// (with the lines that continue it); fenced code, `$$` math, tables and quotes
/// stay one block each. List nesting becomes indentation, two spaces per level,
/// up to three levels.
pub(super) fn blocks(markdown: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut widths: Vec<usize> = vec![];
    let mut item: Option<usize> = None;
    let mut fence: Option<(String, usize, Vec<String>)> = None;
    let mut group: Option<(char, Vec<String>)> = None;
    let flush = |group: &mut Option<(char, Vec<String>)>, out: &mut Vec<String>| {
        if let Some((_, lines)) = group.take() {
            out.push(lines.join("\n"));
        }
    };
    for raw in markdown.lines() {
        let line = raw.replace('\t', "  ");
        let width = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();
        if let Some((marker, indent, lines)) = &mut fence {
            let dedented = line
                .get(*indent..)
                .filter(|_| width >= *indent)
                .unwrap_or(trimmed);
            lines.push(dedented.to_string());
            let closes = if marker == "$$" {
                trimmed.trim_end() == "$$"
            } else {
                trimmed.starts_with(marker.as_str())
                    && trimmed
                        .trim_end()
                        .chars()
                        .all(|c| c == marker.chars().next().unwrap())
            };
            if closes {
                out.push(lines.join("\n"));
                fence = None;
            }
            continue;
        }
        let opens = ["```", "~~~"].iter().find(|m| trimmed.starts_with(*m));
        if opens.is_some() || trimmed.trim_end() == "$$" {
            flush(&mut group, &mut out);
            let marker: String = match opens {
                Some(m) => trimmed.chars().take_while(|c| m.starts_with(*c)).collect(),
                None => "$$".into(),
            };
            fence = Some((marker, width, vec![trimmed.to_string()]));
            item = None;
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut group, &mut out);
            continue;
        }
        let kind = if trimmed.starts_with('|') {
            Some('|')
        } else if trimmed.starts_with('>') {
            Some('>')
        } else {
            None
        };
        if let Some(kind) = kind {
            if group.as_ref().is_some_and(|(k, _)| *k != kind) {
                flush(&mut group, &mut out);
            }
            group
                .get_or_insert((kind, vec![]))
                .1
                .push(trimmed.to_string());
            item = None;
            continue;
        }
        flush(&mut group, &mut out);
        let list = trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed.trim_end() == "-"
            || trimmed.split_once(['.', ')']).is_some_and(|(n, rest)| {
                !n.is_empty()
                    && n.len() <= 9
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && rest.starts_with(' ')
            });
        if list {
            while widths.last().is_some_and(|w| *w >= width) {
                widths.pop();
            }
            let level = widths.len().min(3);
            widths.push(width);
            let text = match trimmed
                .strip_prefix("* ")
                .or_else(|| trimmed.strip_prefix("+ "))
            {
                Some(rest) => format!("- {rest}"),
                None => trimmed.to_string(),
            };
            out.push(format!("{}{}", "  ".repeat(level), text));
            item = Some(width);
            continue;
        }
        // A line indented under a list item continues it.
        if let (Some(w), Some(last)) = (item, out.last_mut()) {
            if width > w {
                last.push('\n');
                last.push_str(&"  ".repeat(indent_of(last)));
                last.push_str(trimmed);
                continue;
            }
        }
        widths.clear();
        item = None;
        out.push(trimmed.to_string());
    }
    if let Some((_, _, lines)) = fence {
        out.push(lines.join("\n"));
    }
    flush(&mut group, &mut out);
    out.retain(|b| !b.trim().is_empty() && b.trim() != "-");
    out
}
fn indent_of(block: &str) -> usize {
    (block.len() - block.trim_start_matches(' ').len()) / 2
}

/// Links and files of the imported pages, for rewriting their Markdown.
struct Links<'a> {
    by_title: HashMap<String, &'a str>,
    by_path: HashMap<String, &'a str>,
    by_name: HashMap<String, String>,
    /// Logseq block IDs: the note holding the block, and the block's text.
    blocks: HashMap<String, (&'a str, String)>,
}
impl Links<'_> {
    fn note(&self, target: &str, from: &str) -> Option<&str> {
        let target = percent_decode(target.split('#').next().unwrap_or(""));
        if target.is_empty() {
            return None;
        }
        if let Some(id) = self.by_path.get(&resolve(from, &target).to_lowercase()) {
            return Some(id);
        }
        let title = target
            .trim_end_matches(".md")
            .rsplit('/')
            .next()
            .unwrap_or("");
        self.by_title
            .get(&without_notion_id(title).to_lowercase())
            .or_else(|| self.by_title.get(&target.to_lowercase()))
            .copied()
    }
    /// The path of a file a page uses: relative to the page, from the export's
    /// root, or (as Obsidian finds them) by file name alone.
    fn file(&self, target: &str, from: &str, files: &HashMap<String, Vec<u8>>) -> Option<String> {
        let target = percent_decode(target.split(['#', '?']).next().unwrap_or(""));
        let target = target.trim_start_matches("./");
        for candidate in [
            resolve(from, target),
            target.trim_start_matches('/').to_string(),
        ] {
            if files.contains_key(&candidate) {
                return Some(candidate);
            }
        }
        let name = target.rsplit('/').next()?.to_lowercase();
        self.by_name.get(&name).cloned()
    }
}
/// `target` relative to the folder of `from`, with `..` resolved.
fn resolve(from: &str, target: &str) -> String {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// Marks a Logseq block ID inside imported Markdown until blocks are made.
const ID_MARK: char = '\u{3}';
fn valid_block_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
/// A block's Markdown without its ID mark, and the ID it marks.
fn block_id(markdown: &str) -> (String, Option<String>) {
    let Some(start) = markdown.find(ID_MARK) else {
        return (markdown.to_string(), None);
    };
    let after = &markdown[start + ID_MARK.len_utf8()..];
    let Some(end) = after.find(ID_MARK) else {
        return (markdown.replace(ID_MARK, ""), None);
    };
    let id = after[..end].to_string();
    let rest = format!(
        "{}{}",
        &markdown[..start],
        &after[end + ID_MARK.len_utf8()..]
    );
    // Any other marks (a block holding nested ones) go.
    let (rest, _) = block_id(&rest);
    (rest, Some(id))
}
/// Rewrites a page's links: `[[wiki links]]` and relative links to imported notes,
/// embedded and linked files to attachments (via `attach`), Logseq block references
/// to links, and Evernote's media. Fenced code is left as it is.
fn rewrite(
    markdown: &str,
    from: &str,
    links: &Links,
    files: &HashMap<String, Vec<u8>>,
    attach: &mut dyn FnMut(&str) -> Option<String>,
) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut fenced = false;
    for line in markdown.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
        }
        // Logseq's own block bookkeeping. A block's ID is kept, as a mark on its
        // first line that becomes the Minerva block's ID (see `block_id`), so
        // references and embeds of it still reach it.
        let bare = t.trim_end();
        if !fenced {
            if let Some(id) = bare.strip_prefix("id:: ").map(str::trim) {
                if valid_block_id(id) && out.ends_with('\n') {
                    out.pop();
                    out.push_str(&format!("{ID_MARK}{id}{ID_MARK}\n"));
                }
                continue;
            }
            if bare == "collapsed:: true" || bare == "collapsed:: false" {
                continue;
            }
        }
        if fenced || t.starts_with("```") || t.starts_with("~~~") {
            out.push_str(line);
            continue;
        }
        out.push_str(&rewrite_line(line, from, links, files, attach));
    }
    out
}
fn rewrite_line(
    line: &str,
    from: &str,
    links: &Links,
    files: &HashMap<String, Vec<u8>>,
    attach: &mut dyn FnMut(&str) -> Option<String>,
) -> String {
    let mut out = String::new();
    let mut rest = line;
    while !rest.is_empty() {
        // Evernote media placeholders.
        if let Some(after) = rest.strip_prefix("\u{1}media:") {
            if let Some((hash, tail)) = after.split_once('\u{1}') {
                out.push_str(&attach(&format!("evernote:{hash}")).unwrap_or_default());
                rest = tail;
                continue;
            }
        }
        // `((block id))`: a link to that block.
        if let Some(after) = rest.strip_prefix("((") {
            if let Some((id, tail)) = after.split_once("))") {
                if let Some((note, text)) = links.blocks.get(id.trim()) {
                    out.push_str(&format!("[{}](#note/{note}#{})", label(text), id.trim()));
                    rest = tail;
                    continue;
                }
            }
        }
        // `{{embed ((block id))}}` and `{{embed [[page]]}}`: embeds.
        if let Some(after) = rest.strip_prefix("{{embed ") {
            if let Some((inner, tail)) = after.split_once("}}") {
                let inner = inner.trim();
                let embed =
                    if let Some(id) = inner.strip_prefix("((").and_then(|i| i.strip_suffix("))")) {
                        links.blocks.get(id.trim()).map(|(note, text)| {
                            format!("![{}](#note/{note}#{})", label(text), id.trim())
                        })
                    } else if let Some(page) =
                        inner.strip_prefix("[[").and_then(|i| i.strip_suffix("]]"))
                    {
                        links
                            .note(page, from)
                            .map(|note| format!("![{}](#note/{note})", label(page)))
                    } else {
                        None
                    };
                if let Some(embed) = embed {
                    out.push_str(&embed);
                    rest = tail;
                    continue;
                }
            }
        }
        let embed = rest.starts_with("![[");
        if embed || rest.starts_with("[[") {
            let after = &rest[if embed { 3 } else { 2 }..];
            if let Some((inner, tail)) = after.split_once("]]") {
                let (target, alias) = inner.split_once('|').unwrap_or((inner, ""));
                let shown = if alias.is_empty() || alias.bytes().all(|b| b.is_ascii_digit()) {
                    target.split('#').next().unwrap_or(target)
                } else {
                    alias
                };
                if let Some(file) = links
                    .file(target, from, files)
                    .filter(|_| !target.ends_with(".md"))
                {
                    out.push_str(&attach(&file).unwrap_or_else(|| shown.to_string()));
                } else if let Some(note) = links.note(target, from) {
                    out.push_str(&format!("[{}](#note/{note})", label(shown)));
                } else {
                    out.push_str(shown);
                }
                rest = tail;
                continue;
            }
        }
        // `[label](target)` and `![alt](target)` to local files and pages.
        let image = rest.starts_with("![");
        if image || rest.starts_with('[') {
            let open = if image { 2 } else { 1 };
            if let Some(close) = rest[open..].find("](").map(|i| i + open) {
                let text = &rest[open..close];
                let after = &rest[close + 2..];
                let mut depth = 0;
                let end = after.char_indices().find(|&(_, c)| {
                    match c {
                        '(' => depth += 1,
                        ')' if depth == 0 => return true,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    false
                });
                if let (false, Some((end, _))) = (text.contains('['), end) {
                    let target = after[..end]
                        .trim()
                        .trim_start_matches('<')
                        .trim_end_matches('>');
                    let target = target.split(" \"").next().unwrap_or(target);
                    let local = !target.contains("://")
                        && !target.starts_with('#')
                        && !target.starts_with("mailto:")
                        && !target.is_empty();
                    if local {
                        let rewritten = if target.to_lowercase().ends_with(".md") {
                            links
                                .note(target, from)
                                .map(|n| format!("[{}](#note/{n})", label(text)))
                        } else {
                            links.file(target, from, files).and_then(|f| attach(&f))
                        };
                        if let Some(rewritten) = rewritten {
                            out.push_str(&rewritten);
                            rest = &after[end + 1..];
                            continue;
                        }
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}
fn label(text: &str) -> String {
    let text: String = text
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | '\n'))
        .collect();
    let text = text.trim();
    if text.is_empty() {
        "link".into()
    } else {
        text.chars().take(200).collect()
    }
}

/// Creates the folders, notes, attachments and databases of an export.
fn apply(s: &mut Store, u: &User, target: &str, mut export: Export) -> Result<Value, ApiError> {
    if export.pages.is_empty() && export.tables.is_empty() {
        return Err(bad(
            "No Markdown pages, Evernote notes or tables were found in this file",
        ));
    }
    let mut d = s.data.clone();
    let workspace = d
        .folders
        .iter()
        .find(|f| f.id == target)
        .map(|f| f.workspace_id.clone())
        .ok_or_else(missing)?;
    // Folders like the export's, when the person may add folders.
    let nested = edits_workspace(&d, u, &workspace);
    let mut folders: HashMap<Vec<String>, String> = HashMap::from([(vec![], target.to_string())]);
    let mut created_folders = 0;
    let mut folder_for = |d: &mut Data, dir: &[String]| -> String {
        if !nested {
            return target.to_string();
        }
        for depth in 1..=dir.len() {
            let key = dir[..depth].to_vec();
            if !folders.contains_key(&key) {
                let parent = folders[&dir[..depth - 1].to_vec()].clone();
                let id = id();
                d.folders.push(Folder {
                    id: id.clone(),
                    workspace_id: workspace.clone(),
                    name: dir[depth - 1].chars().take(120).collect(),
                    parent_id: Some(parent),
                });
                created_folders += 1;
                folders.insert(key, id);
            }
        }
        folders[dir].clone()
    };
    if !nested && export.pages.iter().any(|p| !p.dir.is_empty()) {
        export.warnings.push(
            "You cannot add folders in this workspace, so every note went into the chosen folder."
                .into(),
        );
    }
    // Note IDs first, so that pages can link to each other.
    let ids: Vec<String> = export.pages.iter().map(|_| id()).collect();
    let mut by_title = HashMap::new();
    let mut by_path = HashMap::new();
    for (p, id) in export.pages.iter().zip(&ids) {
        by_title
            .entry(p.title.to_lowercase())
            .or_insert(id.as_str());
        if !p.path.is_empty() {
            by_path.insert(p.path.to_lowercase(), id.as_str());
        }
    }
    let mut by_name = HashMap::new();
    for path in export.files.keys() {
        if let Some(name) = path.rsplit('/').next() {
            by_name
                .entry(name.to_lowercase())
                .or_insert_with(|| path.clone());
        }
    }
    let mut blocks_by_id = HashMap::new();
    for (p, id) in export.pages.iter().zip(&ids) {
        let mut previous = "";
        for line in p.markdown.lines() {
            match line.trim().strip_prefix("id:: ") {
                Some(block) => {
                    let text = previous.trim().trim_start_matches("- ").to_string();
                    blocks_by_id.insert(block.trim().to_string(), (id.as_str(), text));
                }
                None => previous = line,
            }
        }
    }
    let links = Links {
        by_title,
        by_path,
        by_name,
        blocks: blocks_by_id,
    };
    let (mut notes, mut attachments, mut failed_files) = (0, 0, vec![]);
    let mut first = None;
    for (p, id) in export.pages.iter().zip(&ids) {
        let mut stored: HashMap<String, String> = HashMap::new();
        let mut attach = |key: &str| -> Option<String> {
            if let Some(markdown) = stored.get(key) {
                return Some(markdown.clone());
            }
            let bytes = export.files.get(key)?;
            let name = export
                .names
                .get(key)
                .cloned()
                .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(key).to_string());
            let attachment = super::id();
            match attachments::store(s, id, &attachment, &name, bytes) {
                Ok((name, media)) => {
                    attachments += 1;
                    let shown = label(&name);
                    let markdown = if media.starts_with("image/") {
                        format!("![{shown}](/api/v1/attachments/{attachment})")
                    } else {
                        format!("[{shown}](/api/v1/attachments/{attachment})")
                    };
                    stored.insert(key.into(), markdown.clone());
                    Some(markdown)
                }
                Err(e) => {
                    failed_files.push(format!("{name} ({})", e.1));
                    None
                }
            }
        };
        let markdown = rewrite(&p.markdown, &p.path, &links, &export.files, &mut attach);
        let mut parts = blocks(&markdown);
        if parts.len() > BLOCKS {
            export
                .warnings
                .push(format!("“{}” was cut to its first 2,000 blocks", p.title));
            parts.truncate(BLOCKS);
        }
        if parts.is_empty() {
            parts.push(String::new());
        }
        let folder = folder_for(&mut d, &p.dir);
        d.notes.push(Arc::new(Note {
            id: id.clone(),
            folder_id: folder,
            title: if p.title.trim().is_empty() {
                "Imported note".into()
            } else {
                p.title.clone()
            },
            icon: String::new(),
            blocks: {
                let mut used = HashSet::new();
                parts
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        let (markdown, own) = block_id(b);
                        let mut block = Block::new(&truncate(&markdown, BLOCK_BYTES), i);
                        // Logseq's ID, so references reach the block.
                        if let Some(own) = own.filter(|o| used.insert(o.clone())) {
                            block.id = own;
                        }
                        block
                    })
                    .collect()
            },
            revision: 1,
            updated_at: now(),
            favorite: false,
            trashed: false,
            archived: false,
            cover: String::new(),
            tags: p.tags.clone(),
            journal: None,
            metadata: p.metadata.clone(),
            print_template: String::new(),
            record: None,
            folded: None,
        }));
        first.get_or_insert(id.clone());
        notes += 1;
    }
    let mut databases = 0;
    let mut pending = vec![];
    let mut created = vec![];
    for t in &export.tables {
        let folder = folder_for(&mut d, &t.dir);
        let mut mine = vec![];
        if let Some(db) = database(t, &folder, &mut export.warnings, &mut mine) {
            created.push(db.id.clone());
            d.databases.push(db);
            pending.append(&mut mine);
            databases += 1;
        }
    }
    link_relations(&mut d, &created, pending);
    persist(s, d)?;
    if !failed_files.is_empty() {
        export.warnings.push(format!(
            "{} file{} could not be added: {}",
            failed_files.len(),
            if failed_files.len() == 1 { "" } else { "s" },
            failed_files
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(json!({
        "notes": notes,
        "folders": created_folders,
        "databases": databases,
        "attachments": attachments,
        "first_note": first,
        "warnings": export.warnings,
    }))
}
fn truncate(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_string();
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// A Notion table as a database: the first column is the name; other columns
/// become checkboxes, numbers, dates, links or choices when all their values fit.
fn database(
    t: &Table,
    folder: &str,
    warnings: &mut Vec<String>,
    pending: &mut Vec<PendingRelation>,
) -> Option<Database> {
    let header = t.rows.first()?;
    if header.len() > 32 {
        warnings.push(format!(
            "“{}”: only its first 32 columns were imported",
            t.title
        ));
    }
    let width = header.len().min(32);
    let rows: Vec<&Vec<String>> = t.rows[1..].iter().take(10_000).collect();
    if t.rows.len() > 10_001 {
        warnings.push(format!(
            "“{}”: only its first 10,000 rows were imported",
            t.title
        ));
    }
    let mut columns: Vec<Column> = vec![];
    let mut names = HashSet::new();
    for (i, raw) in header.iter().take(width).enumerate() {
        let mut name: String = raw
            .trim()
            .trim_start_matches('\u{feff}')
            .chars()
            .take(100)
            .collect();
        if name.is_empty() {
            name = format!("Column {}", i + 1);
        }
        while !names.insert(name.clone()) {
            name.push('′');
        }
        let values: Vec<&str> = rows
            .iter()
            .filter_map(|r| r.get(i))
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect();
        let (kind, options) = if i == 0 {
            (ColumnKind::Text, vec![])
        } else {
            column_kind(&values)
        };
        columns.push(Column {
            id: id(),
            name,
            kind,
            options,
            target: None,
            rollup: None,
            formula: String::new(),
            format: String::new(),
        });
    }
    let db_id = id();
    // Relations are linked once every table exists; until then, text.
    let mut relations: Vec<PendingRelation> = columns
        .iter()
        .filter(|c| c.kind == ColumnKind::Relation)
        .map(|c| PendingRelation {
            database: db_id.clone(),
            column: c.id.clone(),
            cells: vec![],
        })
        .collect();
    for c in columns
        .iter_mut()
        .filter(|c| c.kind == ColumnKind::Relation)
    {
        c.kind = ColumnKind::Text;
    }
    let relation_ids: Vec<String> = relations.iter().map(|r| r.column.clone()).collect();
    let records = rows
        .iter()
        .map(|row| {
            let record = id();
            let mut cells = BTreeMap::new();
            for (c, value) in columns.iter().zip(row.iter()) {
                let value = value.trim();
                if value.is_empty() {
                    continue;
                }
                if relation_ids.contains(&c.id) {
                    if let Some(p) = relations.iter_mut().find(|p| p.column == c.id) {
                        p.cells.push((record.clone(), related_items(value)));
                    }
                    continue;
                }
                let cell = match c.kind {
                    ColumnKind::Checkbox => json!(value == "Yes"),
                    ColumnKind::Number => json!(value.parse::<f64>().unwrap_or(0.0)),
                    ColumnKind::Date => json!(date(value).unwrap_or_default()),
                    ColumnKind::MultiSelect => json!(value
                        .split(", ")
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                        .collect::<Vec<_>>()),
                    _ => json!(truncate(value, 10_000)),
                };
                cells.insert(c.id.clone(), cell);
            }
            Record {
                id: record,
                cells,
                revision: 1,
                updated_at: now(),
                created_at: now(),
                archived: false,
                conflict_of: None,
                rank: None,
            }
        })
        .collect();
    pending.append(&mut relations);
    let db = Database {
        id: db_id,
        folder_id: folder.into(),
        name: if t.title.trim().is_empty() {
            "Imported table".into()
        } else {
            t.title.chars().take(200).collect()
        },
        columns,
        rows: records,
        revision: 1,
        updated_at: now(),
    };
    databases::validate_columns(&db.columns).ok()?;
    Some(db)
}
fn column_kind(values: &[&str]) -> (ColumnKind, Vec<String>) {
    if values.is_empty() {
        return (ColumnKind::Text, vec![]);
    }
    if values.iter().all(|v| matches!(*v, "Yes" | "No")) {
        return (ColumnKind::Checkbox, vec![]);
    }
    if values
        .iter()
        .all(|v| v.parse::<f64>().is_ok_and(f64::is_finite))
    {
        return (ColumnKind::Number, vec![]);
    }
    if values.iter().all(|v| date(v).is_some()) {
        return (ColumnKind::Date, vec![]);
    }
    if values.iter().all(|v| {
        (v.starts_with("https://") || v.starts_with("http://"))
            && !v.contains(' ')
            && v.len() <= 2048
    }) {
        return (ColumnKind::Url, vec![]);
    }
    // Notion's relations: `Title (path/Title id.md)`, several joined by ", ".
    if values.iter().all(|v| !related_items(v).is_empty()) {
        return (ColumnKind::Relation, vec![]);
    }
    // Notion's multi-select: options joined by ", ", some used more than once.
    if values.iter().any(|v| v.contains(", ")) {
        let mut options: Vec<String> = vec![];
        let mut used = 0;
        for v in values {
            for part in v.split(", ").map(str::trim).filter(|p| !p.is_empty()) {
                used += 1;
                if !options.iter().any(|o| o == part) {
                    options.push(part.to_string());
                }
            }
        }
        if options.len() <= 50 && options.len() < used && options.iter().all(|o| o.len() <= 100) {
            return (ColumnKind::MultiSelect, options);
        }
    }
    let mut options: Vec<String> = vec![];
    for v in values {
        if !options.iter().any(|o| o == v) {
            options.push(v.to_string());
        }
    }
    let choices = options.len() <= 20
        && options.len() < values.len()
        && options.iter().all(|o| o.len() <= 100 && !o.contains(", "));
    if choices {
        (ColumnKind::Select, options)
    } else {
        (ColumnKind::Text, vec![])
    }
}
/// The items of a Notion relation cell: each page's title and the folder its
/// file is in (the related table's name). Empty when it is not one.
fn related_items(value: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut rest = value.trim();
    while !rest.is_empty() {
        // `Title (path.md)`, the path percent-encoded.
        let Some(open) = rest.find(" (") else {
            return vec![];
        };
        let Some(close) = rest[open..].find(".md)").map(|i| open + i + 4) else {
            return vec![];
        };
        let title = rest[..open].trim().to_string();
        let path = percent_decode(&rest[open + 2..close - 1]);
        let folder = path
            .rsplit_once('/')
            .map(|(dir, _)| dir.rsplit('/').next().unwrap_or(dir))
            .map(|f| without_notion_id(f).to_string())
            .unwrap_or_default();
        if title.is_empty() || folder.is_empty() {
            return vec![];
        }
        out.push((title, folder));
        rest = rest[close..].trim_start_matches(',').trim_start();
    }
    out
}
/// A relation column waiting for the other tables: by record, the titles of the
/// related records and the table they are in.
struct PendingRelation {
    database: String,
    column: String,
    cells: Vec<(String, Vec<(String, String)>)>,
}
/// Links relation columns to the tables imported with them; a column whose
/// table is not there stays text.
fn link_relations(d: &mut Data, created: &[String], pending: Vec<PendingRelation>) {
    for p in pending {
        let target_name = p
            .cells
            .iter()
            .flat_map(|(_, items)| items)
            .map(|(_, t)| t.clone())
            .next();
        let target = target_name.and_then(|name| {
            d.databases
                .iter()
                .find(|db| created.contains(&db.id) && db.name.eq_ignore_ascii_case(&name))
                .map(|db| {
                    (
                        db.id.clone(),
                        db.rows
                            .iter()
                            .map(|r| (databases::record_title(db, r), r.id.clone()))
                            .collect::<Vec<_>>(),
                    )
                })
        });
        let Some(db) = d.databases.iter_mut().find(|db| db.id == p.database) else {
            continue;
        };
        let Some(col) = db.columns.iter_mut().find(|c| c.id == p.column) else {
            continue;
        };
        match target {
            Some((target, titles)) => {
                col.kind = ColumnKind::Relation;
                col.target = Some(target);
                for (rid, items) in &p.cells {
                    let ids: Vec<Value> = items
                        .iter()
                        .filter_map(|(title, _)| {
                            titles
                                .iter()
                                .find(|(t, _)| t == title)
                                .map(|(_, id)| json!(id))
                        })
                        .collect();
                    if let Some(r) = db.rows.iter_mut().find(|r| &r.id == rid) {
                        if ids.is_empty() {
                            r.cells.remove(&p.column);
                        } else {
                            r.cells.insert(p.column.clone(), json!(ids));
                        }
                    }
                }
            }
            None => {
                // The titles, as text.
                col.kind = ColumnKind::Text;
                for (rid, items) in &p.cells {
                    if let Some(r) = db.rows.iter_mut().find(|r| &r.id == rid) {
                        let text = items
                            .iter()
                            .map(|(t, _)| t.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        r.cells.insert(p.column.clone(), json!(text));
                    }
                }
            }
        }
    }
}
/// A date as `YYYY-MM-DD`, from `2026-11-01`, `2026/11/01`, or Notion's
/// `November 1, 2026` (a time or the end of a range after it is ignored).
fn date(value: &str) -> Option<String> {
    let value = value.split(" → ").next()?.trim();
    let iso = value.get(..10).map(|v| v.replace('/', "-"));
    if let Some(iso) = iso.filter(|v| databases::valid_date(v)) {
        return Some(iso);
    }
    const MONTHS: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let mut words = value.split_whitespace();
    let month = words.next()?.to_lowercase();
    let month = MONTHS
        .iter()
        .position(|m| m.starts_with(&month) && month.len() >= 3)?
        + 1;
    let day: u32 = words.next()?.trim_end_matches(',').parse().ok()?;
    let year: u32 = words.next()?.parse().ok()?;
    let iso = format!("{year:04}-{month:02}-{day:02}");
    databases::valid_date(&iso).then_some(iso)
}

/// An Evernote export: each note with its tags, and its files by content hash.
fn evernote(xml: &str) -> Result<Export, ApiError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use md5::{Digest, Md5};
    let mut export = Export::default();
    for note in sections(xml, "note") {
        if export.pages.len() >= PAGES {
            export
                .warnings
                .push("Only the first 5,000 notes were imported".into());
            break;
        }
        let title = sections(note, "title")
            .first()
            .map(|t| entities(t))
            .unwrap_or_default();
        let tags: Vec<String> = sections(note, "tag")
            .iter()
            .filter_map(|t| render::normalize_tag(&entities(t).replace(' ', "-")))
            .take(20)
            .collect();
        for resource in sections(note, "resource") {
            let Some(data) = sections(resource, "data")
                .first()
                .map(|d| d.chars().filter(|c| !c.is_whitespace()).collect::<String>())
            else {
                continue;
            };
            let Ok(bytes) = STANDARD.decode(data.as_bytes()) else {
                continue;
            };
            let hash = format!("{:x}", Md5::digest(&bytes));
            let key = format!("evernote:{hash}");
            if let Some(name) = sections(resource, "file-name").first() {
                export.names.insert(key.clone(), entities(name));
            }
            export.files.insert(key, bytes);
        }
        let content = sections(note, "content").first().copied().unwrap_or("");
        let content = content.trim();
        let content = content
            .strip_prefix("<![CDATA[")
            .and_then(|c| c.strip_suffix("]]>"))
            .unwrap_or(content);
        let markdown = html_to_markdown(content);
        let mut metadata = BTreeMap::new();
        if let Some(created) = sections(note, "created").first() {
            if created.len() >= 8 {
                metadata.insert(
                    "created".into(),
                    format!("{}-{}-{}", &created[..4], &created[4..6], &created[6..8]),
                );
            }
        }
        if let Some(source) = sections(note, "source-url").first() {
            metadata.insert("source".into(), entities(source));
        }
        export.pages.push(Page {
            dir: vec![],
            path: String::new(),
            title: if title.trim().is_empty() {
                "Untitled".into()
            } else {
                title.chars().take(200).collect()
            },
            markdown,
            tags,
            metadata,
        });
    }
    Ok(export)
}
/// Bear's tags: `#multi word tag#` (and `#tag#`) become `#multi-word-tag`, as
/// Minerva writes tags. A `#` must start the tag after a space or a line's
/// start, and end it right after a word, so headings and `C#` are left alone.
fn bear_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
        }
        if fenced || !line.contains('#') {
            out.push_str(line);
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let starts = chars[i] == '#'
                && (i == 0 || chars[i - 1].is_whitespace())
                && chars
                    .get(i + 1)
                    .is_some_and(|c| !c.is_whitespace() && *c != '#');
            if starts {
                // The closing `#`, on the same line, right after a word.
                let close = (i + 2..chars.len()).find(|&j| chars[j] == '#');
                if let Some(j) = close {
                    let inner: String = chars[i + 1..j].iter().collect();
                    let ends_word = !chars[j - 1].is_whitespace();
                    let after_ok = chars
                        .get(j + 1)
                        .is_none_or(|c| c.is_whitespace() || ",.;:!?)".contains(*c));
                    if ends_word
                        && after_ok
                        && inner.len() <= 60
                        && !inner.contains(['[', ']', '(', ')'])
                    {
                        let tag: String = inner.split_whitespace().collect::<Vec<_>>().join("-");
                        out.push('#');
                        out.push_str(&tag);
                        i = j + 1;
                        continue;
                    }
                }
            }
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// OneNote's "Single File Web Page" (`.mht`): the page as HTML, with its
/// pictures as parts. They become a note, and its files.
fn mhtml(name: &str, bytes: &[u8]) -> Result<Export, ApiError> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let text = String::from_utf8_lossy(bytes);
    let boundary = text
        .lines()
        .take(60)
        .find_map(|l| {
            let at = l.to_lowercase().find("boundary=")?;
            Some(
                l[at + 9..]
                    .trim()
                    .trim_matches(';')
                    .trim_matches('"')
                    .to_string(),
            )
        })
        .filter(|b| !b.is_empty())
        .ok_or_else(|| bad("This web archive cannot be read"))?;
    let marker = format!("--{boundary}");
    let mut html = None;
    let mut images: Vec<(Vec<String>, Vec<u8>, String)> = vec![];
    for part in text.split(&marker).skip(1) {
        let part = part.trim_start_matches(['\r', '\n']);
        let Some(split) = part
            .find("\r\n\r\n")
            .map(|i| (i, 4))
            .or_else(|| part.find("\n\n").map(|i| (i, 2)))
        else {
            continue;
        };
        let (head, body) = (&part[..split.0], &part[split.0 + split.1..]);
        let header = |name: &str| -> String {
            head.lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.trim()
                        .eq_ignore_ascii_case(name)
                        .then(|| v.trim().to_string())
                })
                .unwrap_or_default()
        };
        let kind = header("Content-Type").to_lowercase();
        let encoding = header("Content-Transfer-Encoding").to_lowercase();
        let raw: Vec<u8> = if encoding.contains("base64") {
            let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            STANDARD
                .decode(clean.trim_end_matches('-'))
                .unwrap_or_default()
        } else if encoding.contains("quoted-printable") {
            quoted_printable(body)
        } else {
            body.as_bytes().to_vec()
        };
        if kind.starts_with("text/html") && html.is_none() {
            // OneNote writes Windows-1252 at times: those bytes are read as Latin-1.
            html = Some(if kind.contains("1252") || kind.contains("8859-1") {
                raw.iter().map(|&b| b as char).collect::<String>()
            } else {
                String::from_utf8_lossy(&raw).into_owned()
            });
        } else if kind.starts_with("image/") && !raw.is_empty() {
            let mut names = vec![];
            let location = header("Content-Location");
            if !location.is_empty() {
                names.push(location.clone());
            }
            let cid = header("Content-ID");
            if !cid.is_empty() {
                names.push(format!("cid:{}", cid.trim_matches(['<', '>'])));
            }
            let file = location
                .rsplit(['/', '\\'])
                .next()
                .filter(|f| !f.is_empty())
                .unwrap_or("picture")
                .to_string();
            images.push((names, raw, file));
        }
    }
    let mut html = html.ok_or_else(|| bad("This web archive has no page"))?;
    let mut export = Export::default();
    for (i, (names, raw, file)) in images.into_iter().enumerate() {
        let key = format!("mht{i}");
        for n in names {
            for quote in ['"', '\''] {
                html = html.replace(
                    &format!("src={quote}{n}{quote}"),
                    &format!("src=\"mht:{key}\""),
                );
            }
        }
        export.names.insert(format!("evernote:{key}"), file);
        export.files.insert(format!("evernote:{key}"), raw);
    }
    let title = sections(&html, "title")
        .first()
        .map(|t| entities(t).trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            let stem = name.rsplit('/').next().unwrap_or(name);
            stem.rsplit_once('.').map_or(stem, |(s, _)| s).to_string()
        });
    let body = sections(&html, "body")
        .first()
        .copied()
        .unwrap_or(&html)
        .to_string();
    export.pages.push(Page {
        dir: vec![],
        path: String::new(),
        title: title.chars().take(200).collect(),
        markdown: html_to_markdown(&body),
        tags: vec![],
        metadata: BTreeMap::new(),
    });
    Ok(export)
}
/// Quoted-printable text as bytes: `=XX` and soft line breaks.
fn quoted_printable(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let Some(b) = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}
/// The contents of every `<name …>…</name>` in `xml`, not nested in one another.
fn sections<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut found = vec![];
    let mut at = 0;
    while let Some(start) = xml[at..].find(&open).map(|i| i + at) {
        let after = &xml[start + open.len()..];
        // `<note>` but not `<notebook>`.
        if !after.starts_with(['>', ' ', '\n', '\t', '/']) {
            at = start + open.len();
            continue;
        }
        let Some(body) = after.find('>').map(|i| start + open.len() + i + 1) else {
            break;
        };
        if xml[..body].ends_with("/>") {
            at = body;
            continue;
        }
        let Some(end) = xml[body..].find(&close).map(|i| i + body) else {
            break;
        };
        found.push(&xml[body..end]);
        at = end + close.len();
    }
    found
}
fn entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Evernote's note HTML (ENML) as Markdown: paragraphs, headings, emphasis,
/// links, lists, checkboxes, quotes, code, tables and rules; embedded files
/// become placeholders (`\u{1}media:<hash>\u{1}`) for `rewrite`.
pub(super) fn html_to_markdown(html: &str) -> String {
    let mut w = Writer::default();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->").map_or("", |(_, t)| t);
            continue;
        }
        if rest.starts_with('<') {
            let Some(end) = rest.find('>') else { break };
            w.tag(&rest[1..end]);
            rest = &rest[end + 1..];
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        w.text(&entities(&rest[..end]));
        rest = &rest[end..];
    }
    w.finish()
}
#[derive(Default)]
struct Writer {
    out: String,
    lists: Vec<Option<u32>>,
    quote: usize,
    pre: bool,
    link: Option<(String, usize)>,
    table: Option<Vec<Vec<String>>>,
    cell: Option<String>,
    skip: usize,
}
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[at..];
    let quote = rest.chars().next()?;
    let value = if quote == '"' || quote == '\'' {
        rest[1..].split(quote).next()?
    } else {
        rest.split([' ', '/']).next()?
    };
    Some(entities(value))
}
impl Writer {
    fn push(&mut self, text: &str) {
        if let Some(cell) = &mut self.cell {
            cell.push_str(text);
            return;
        }
        if self.out.is_empty() || self.out.ends_with('\n') {
            if text.trim().is_empty() && !self.pre {
                return;
            }
            for _ in 0..self.quote {
                self.out.push_str("> ");
            }
        }
        self.out.push_str(text);
    }
    fn line(&mut self) {
        if let Some(cell) = &mut self.cell {
            cell.push(' ');
            return;
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }
    fn text(&mut self, text: &str) {
        if self.skip > 0 {
            return;
        }
        if self.pre {
            for (i, part) in text.split('\n').enumerate() {
                if i > 0 {
                    self.out.push('\n');
                }
                self.out.push_str(part);
            }
            return;
        }
        let mut collapsed = String::new();
        for c in text.chars() {
            if c.is_whitespace() {
                if !collapsed.ends_with(' ') {
                    collapsed.push(' ');
                }
            } else {
                collapsed.push(c);
            }
        }
        let at_start = self
            .cell
            .as_ref()
            .map_or(self.out.is_empty() || self.out.ends_with('\n'), |c| {
                c.is_empty()
            });
        let collapsed = if at_start {
            collapsed.trim_start()
        } else {
            &collapsed
        };
        if !collapsed.is_empty() {
            self.push(collapsed);
        }
    }
    fn tag(&mut self, raw: &str) {
        let closing = raw.starts_with('/');
        let body = raw.trim_start_matches('/').trim_end_matches('/');
        let name = body
            .split(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .to_lowercase();
        if matches!(
            name.as_str(),
            "style" | "script" | "title" | "head" | "en-crypt"
        ) {
            if closing {
                self.skip = self.skip.saturating_sub(1);
            } else if !raw.ends_with('/') {
                self.skip += 1;
            }
            return;
        }
        if self.skip > 0 {
            return;
        }
        match (name.as_str(), closing) {
            ("br", _) => self.line(),
            ("p" | "div" | "en-note" | "section" | "article" | "header" | "footer", _) => {
                self.line()
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", false) => {
                self.line();
                let level = name[1..].parse().unwrap_or(1);
                self.push(&format!("{} ", "#".repeat(level)));
            }
            ("h1" | "h2" | "h3" | "h4" | "h5" | "h6", true) => self.line(),
            ("b" | "strong", _) => self.push("**"),
            ("i" | "em", _) => self.push("*"),
            ("s" | "strike" | "del", _) => self.push("~~"),
            ("code", _) if !self.pre => self.push("`"),
            ("pre", false) => {
                self.line();
                self.push("```\n");
                self.pre = true;
            }
            ("pre", true) => {
                self.pre = false;
                self.line();
                self.out.push_str("```\n");
            }
            ("blockquote", false) => {
                self.line();
                self.quote += 1;
            }
            ("blockquote", true) => {
                self.line();
                self.quote = self.quote.saturating_sub(1);
            }
            ("hr", _) => {
                self.line();
                self.push("---");
                self.line();
            }
            ("ul", false) => self.lists.push(None),
            ("ol", false) => self.lists.push(Some(0)),
            ("ul" | "ol", true) => {
                self.lists.pop();
                self.line();
            }
            ("li", false) => {
                self.line();
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{n}. ")
                    }
                    _ => "- ".into(),
                };
                self.push(&format!("{}{marker}", "  ".repeat(depth)));
            }
            ("li", true) => self.line(),
            ("en-todo", _) => {
                let checked = attribute(body, "checked").is_some_and(|c| c == "true");
                let at_start = self.out.is_empty() || self.out.ends_with('\n');
                if at_start {
                    self.push(if checked { "- [x] " } else { "- [ ] " });
                } else if self.out.ends_with("- ") {
                    self.out.push_str(if checked { "[x] " } else { "[ ] " });
                } else {
                    self.push(if checked { "☑ " } else { "☐ " });
                }
            }
            ("en-media", _) => {
                if let Some(hash) = attribute(body, "hash") {
                    self.push(&format!("\u{1}media:{hash}\u{1}"));
                }
            }
            ("img", _) => {
                let src = attribute(body, "src").unwrap_or_default();
                // A picture kept in the import (a web archive's).
                if let Some(key) = src.strip_prefix("mht:") {
                    self.push(&format!("\u{1}media:{key}\u{1}"));
                } else if src.starts_with("http") {
                    self.push(&format!("![]({src})"));
                }
            }
            ("a", false) => {
                let href = attribute(body, "href").unwrap_or_default();
                self.push("[");
                let at = self.cell.as_ref().map_or(self.out.len(), String::len);
                self.link = Some((href, at));
            }
            ("a", true) => {
                if let Some((href, _)) = self.link.take() {
                    if href.is_empty() || href.starts_with("evernote:") {
                        self.push("]()");
                    } else {
                        self.push(&format!("]({href})"));
                    }
                }
            }
            ("table", false) => {
                self.line();
                self.table = Some(vec![]);
            }
            ("tr", false) => {
                if let Some(t) = &mut self.table {
                    t.push(vec![]);
                }
            }
            ("td" | "th", false) => self.cell = Some(String::new()),
            ("td" | "th", true) => {
                if let (Some(cell), Some(row)) = (
                    self.cell.take(),
                    self.table.as_mut().and_then(|t| t.last_mut()),
                ) {
                    row.push(cell.trim().replace('|', "\\|"));
                }
            }
            ("table", true) => {
                if let Some(rows) = self.table.take() {
                    let rows: Vec<_> = rows.into_iter().filter(|r| !r.is_empty()).collect();
                    for (i, row) in rows.iter().enumerate() {
                        self.out.push_str(&format!("| {} |\n", row.join(" | ")));
                        if i == 0 {
                            self.out
                                .push_str(&format!("|{}\n", " --- |".repeat(row.len())));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fn finish(self) -> String {
        // Empty links (`[]()`) and their brackets go.
        self.out
            .replace("[]()", "")
            .replace("]()", "]")
            .trim()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_becomes_blocks_as_if_typed() {
        let md = "# Title\n\nIntro line\n- one\n\t- two\n\t\t- three\n\t\t\t\t- deep\n\t\t\t\t  continued\n- [ ] task\n\n```rust\nlet a = 1;\n\nlet b = 2;\n```\n| a | b |\n| - | - |\n> quote\n> more\n1. first\n2) second";
        assert_eq!(
            blocks(md),
            [
                "# Title",
                "Intro line",
                "- one",
                "  - two",
                "    - three",
                "      - deep\n      continued",
                "- [ ] task",
                "```rust\nlet a = 1;\n\nlet b = 2;\n```",
                "| a | b |\n| - | - |",
                "> quote\n> more",
                "1. first",
                "2) second",
            ]
        );
    }
    #[test]
    fn notion_logseq_and_obsidian_names() {
        assert_eq!(
            without_notion_id("Plans 0123456789abcdef0123456789abcdef"),
            "Plans"
        );
        assert_eq!(without_notion_id("Plans 2026"), "Plans 2026");
        let p = page(
            vec!["pages".into()],
            "pages/books___dune.md",
            "books___dune",
            "tags:: scifi, [[Herbert]]\nrating:: 5\n\n- Spice",
        );
        assert_eq!(p.title, "books/dune");
        assert_eq!(p.tags, ["scifi", "herbert"]);
        assert_eq!(p.metadata["rating"], "5");
        assert_eq!(p.markdown, "- Spice");
        let j = page(
            vec!["journals".into()],
            "journals/2026_11_01.md",
            "2026_11_01",
            "- day",
        );
        assert_eq!(j.title, "2026-11-01");
        let o = page(
            vec![],
            "Note.md",
            "Note",
            "---\ntags: [a, b]\naliases:\n  - N\nauthor: Ada\n---\n# Note\nBody",
        );
        assert_eq!(
            (o.title.as_str(), o.tags.clone(), o.markdown.as_str()),
            ("Note", vec!["a".to_string(), "b".to_string()], "Body")
        );
        assert_eq!(o.metadata["author"], "Ada");
    }
    #[test]
    fn evernote_html_becomes_markdown() {
        let html = r#"<en-note><div><b>Bold</b> and <i>it</i> &amp; <a href="https://x.example">link</a></div><div><en-todo checked="true"/>Done</div><ul><li>a<ul><li>b</li></ul></li></ul><ol><li>one</li><li>two</li></ol><h2>Head</h2><en-media hash="abc" type="image/png"/><table><tr><td>x</td><td>y</td></tr><tr><td>1</td><td>2</td></tr></table><blockquote>Said</blockquote><pre>code
  here</pre></en-note>"#;
        assert_eq!(
            html_to_markdown(html),
            "**Bold** and *it* & [link](https://x.example)\n- [x] Done\n- a\n  - b\n1. one\n2. two\n## Head\n\u{1}media:abc\u{1}\n| x | y |\n| --- | --- |\n| 1 | 2 |\n> Said\n```\ncode\n  here\n```"
        );
    }
    #[test]
    fn notion_dates_and_column_kinds() {
        assert_eq!(
            date("November 1, 2026 3:00 PM").as_deref(),
            Some("2026-11-01")
        );
        assert_eq!(
            date("2026/02/03 → 2026/02/05").as_deref(),
            Some("2026-02-03")
        );
        assert_eq!(date("soon"), None);
        assert_eq!(column_kind(&["Yes", "No"]).0, ColumnKind::Checkbox);
        assert_eq!(column_kind(&["1", "2.5"]).0, ColumnKind::Number);
        assert_eq!(
            column_kind(&["Done", "Done", "To do"]),
            (ColumnKind::Select, vec!["Done".into(), "To do".into()])
        );
        // Options joined by commas, used again: Notion's multi-select.
        assert_eq!(
            column_kind(&["a, b", "a, b"]),
            (ColumnKind::MultiSelect, vec!["a".into(), "b".into()])
        );
        // Prose with commas and no repeats stays text.
        assert_eq!(
            column_kind(&["one, two", "three, four"]).0,
            ColumnKind::Text
        );
    }
}
