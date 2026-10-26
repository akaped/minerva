//! Markdown rendering: sanitized HTML, language-aware code highlighting, JSON
//! diagnostics, and attachment URLs mapped for the audience (members, share-link
//! guests, or the public web).
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use uuid::Uuid;

/// Where attachment links in rendered Markdown should point.
#[derive(Clone, Copy)]
pub(crate) enum Links<'a> {
    /// Signed-in members: the authenticated attachment API.
    Members,
    /// Guests of a share link: the share-scoped attachment API.
    Shared,
    /// A published page: files served under the page's path (`p/{slug}` or
    /// `{site}/{slug}`).
    Public(&'a str),
    /// An exported document: member attachment URLs, task states shown as
    /// ☐/☑, and no links to other notes (they only work inside the app).
    Export,
}
impl Links<'_> {
    fn key(&self) -> Cow<'_, str> {
        match self {
            Links::Members => "m".into(),
            Links::Shared => "s".into(),
            Links::Export => "e".into(),
            Links::Public(page) => format!("p:{page}").into(),
        }
    }
    fn map(&self, destination: &str, image: bool) -> Option<String> {
        let (id, absolute) = attachment(destination)?;
        let suffix = if image { "/preview" } else { "" };
        match self {
            // Absolute URLs keep their host; only the preview suffix is added.
            Links::Members if absolute => image.then(|| {
                let mut url = url::Url::parse(destination).ok()?;
                url.set_path(&format!("/api/v1/attachments/{id}/preview"));
                Some(url.to_string())
            })?,
            Links::Members | Links::Export => {
                image.then(|| format!("/api/v1/attachments/{id}/preview"))
            }
            Links::Shared if !absolute => Some(format!("/api/v1/shared/attachments/{id}{suffix}")),
            Links::Public(page) if !absolute => Some(format!("/{page}/files/{id}{suffix}")),
            _ => None,
        }
    }
}
/// Extracts the attachment ID from `/api/v1/attachments/{id}[/preview]`, relative
/// or absolute (HTTP/S). Returns whether the URL was absolute.
pub(crate) fn attachment(destination: &str) -> Option<(Uuid, bool)> {
    let (path, absolute) = match url::Url::parse(destination) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => (url.path().to_string(), true),
        Ok(_) => return None,
        Err(_) => (destination.to_string(), false),
    };
    let rest = path.strip_prefix("/api/v1/attachments/")?;
    let key = rest.strip_suffix("/preview").unwrap_or(rest);
    Uuid::parse_str(key).ok().map(|id| (id, absolute))
}

pub(crate) fn render(markdown: &str) -> String {
    render_for(markdown, Links::Members)
}

pub(crate) fn render_for(markdown: &str, links: Links) -> String {
    // A block holding only a YouTube link plays the video on published pages,
    // which run no scripts; the editor draws its own player.
    if let (Links::Public(_), Some((id, start))) = (links, youtube(markdown)) {
        return youtube_embed(&id, start);
    }
    /// Rendered HTML by audience and Markdown, bounded by total size so that large
    /// or numerous inputs cannot exhaust memory.
    #[derive(Default)]
    struct Cache {
        entries: HashMap<(String, String), String>,
        bytes: usize,
    }
    const CACHE_BYTES: usize = 32 * 1024 * 1024;
    const CACHEABLE: usize = 16 * 1024;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (links.key().into_owned(), markdown.to_string());
    if let Some(html) = cache.lock().unwrap().entries.get(&key) {
        return html.clone();
    }
    let (level, body) = indentation(markdown);
    let mut html = to_html(&body, links);
    if level > 0 {
        html = format!("<div class=\"md-indent md-indent-{level}\">{html}</div>");
    }
    let mut html = clean(&html);
    // Outside the app (published pages, share links, exports), a mention
    // shows the name part only: members' addresses are not published.
    if !matches!(links, Links::Members) {
        html = private_mentions(&html);
    }
    if markdown.len() <= CACHEABLE {
        let mut cache = cache.lock().unwrap();
        let size = key.0.len() + key.1.len() + html.len();
        // Notes are re-rendered on every sync poll; start over when the budget is spent.
        if cache.bytes + size > CACHE_BYTES {
            cache.entries.clear();
            cache.bytes = 0;
        }
        cache.bytes += size;
        cache.entries.insert(key, html.clone());
    }
    html
}

/// Rewrites mention chips to `@name`, the part of the address before the `@`.
fn private_mentions(html: &str) -> String {
    const OPEN: &str = "<span class=\"mention\" title=\"Assigned to ";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let after = &rest[at + OPEN.len()..];
        let Some(end) = after.find("</span>") else {
            out.push_str(&rest[at..]);
            return out;
        };
        let address = after[..end].split('"').next().unwrap_or("");
        let name = address.split('@').next().unwrap_or("");
        out.push_str(&format!("<span class=\"mention\">@{name}</span>"));
        rest = &after[end + "</span>".len()..];
    }
    out.push_str(rest);
    out
}

/// The video ID and start time of a block that holds only a YouTube link,
/// following the editor's rules (`web/youtube.js`).
pub(crate) fn youtube(markdown: &str) -> Option<(String, u32)> {
    let raw = markdown.trim();
    if raw.chars().any(char::is_whitespace) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let host = url.host_str()?.to_lowercase();
    let parts: Vec<&str> = url.path_segments()?.filter(|p| !p.is_empty()).collect();
    let query = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    };
    let id = match (host.as_str(), parts.as_slice()) {
        ("youtu.be", [id]) => id.to_string(),
        (
            "youtube.com"
            | "www.youtube.com"
            | "m.youtube.com"
            | "youtube-nocookie.com"
            | "www.youtube-nocookie.com",
            _,
        ) if url.path() == "/watch" => query("v")?,
        (
            "youtube.com"
            | "www.youtube.com"
            | "m.youtube.com"
            | "youtube-nocookie.com"
            | "www.youtube-nocookie.com",
            [kind, id],
        ) if matches!(*kind, "embed" | "shorts" | "live") => id.to_string(),
        _ => return None,
    };
    if id.len() != 11
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let time = query("t").or_else(|| query("start")).unwrap_or_default();
    let start = if let Ok(seconds) = time.parse::<u32>() {
        seconds
    } else {
        // `1h2m3s`, `2m`, `45s`.
        let (mut total, mut number) = (0u32, String::new());
        for c in time.chars() {
            match c {
                '0'..='9' => number.push(c),
                'h' | 'm' | 's' => {
                    let unit = match c {
                        'h' => 3600,
                        'm' => 60,
                        _ => 1,
                    };
                    total = total.saturating_add(number.parse::<u32>().ok()?.saturating_mul(unit));
                    number.clear();
                }
                _ => return Some((id, 0)),
            }
        }
        if number.is_empty() {
            total
        } else {
            0
        }
    };
    Some((id, start.min(86400)))
}
/// The player for a published page. The ID is validated above, so it is safe in
/// markup. The iframe sends its origin as referrer, which YouTube requires, even
/// though pages otherwise send none to other sites.
fn youtube_embed(id: &str, start: u32) -> String {
    let start = if start > 0 {
        format!("?start={start}")
    } else {
        String::new()
    };
    format!(
        "<div class=\"youtube-embed\"><iframe src=\"https://www.youtube-nocookie.com/embed/{id}{start}\" title=\"YouTube video\" loading=\"lazy\" referrerpolicy=\"strict-origin-when-cross-origin\" allow=\"encrypted-media; fullscreen; picture-in-picture\" allowfullscreen></iframe></div><p class=\"youtube-link\"><a href=\"https://www.youtube.com/watch?v={id}\" rel=\"noopener noreferrer\">Watch on YouTube</a></p>"
    )
}

/// Blocks nest under the block above with two leading spaces per level (Tab in the
/// editor), up to six levels. The indentation is removed before parsing, so four
/// spaces never turn a paragraph into a code block, and returned as the level.
pub(crate) fn indentation(markdown: &str) -> (usize, String) {
    let spaces = |line: &str| line.len() - line.trim_start_matches(' ').len();
    let Some(first) = markdown.lines().find(|l| !l.trim().is_empty()) else {
        return (0, markdown.to_string());
    };
    let level = (spaces(first) / 2).min(6);
    if level == 0 {
        return (0, markdown.to_string());
    }
    let strip = level * 2;
    let body = markdown
        .split('\n')
        .map(|line| &line[spaces(line).min(strip)..])
        .collect::<Vec<_>>()
        .join("\n");
    (level, body)
}

/// Stands in for `[[uncompleted_journal_tasks]]` while parsing (a private-use
/// character cannot be typed as Markdown syntax).
const TASKS_SENTINEL: &str = "\u{E000}journal-tasks\u{E000}";
/// `[[database:{id}]]` or `[[database:{id}:board]]` embeds a live database view.
/// While parsing it becomes `\u{E001}{id}[:view]\u{E001}`.
pub(crate) const DATABASE_EMBED: &str = "[[database:";
const DB_MARK: char = '\u{E001}';
/// The ID and view of a database embed body (`{id}` or `{id}:{view}`).
pub(crate) fn database_embed(body: &str) -> Option<(&str, &str)> {
    let (id, view) = body.split_once(':').unwrap_or((body, "table"));
    let valid_id =
        (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    (valid_id && matches!(view, "table" | "board")).then_some((id, view))
}
/// Calls `replace` with each well-formed database embed body and splices in what
/// it returns.
pub(crate) fn replace_database_embeds(
    markdown: &str,
    mut replace: impl FnMut(&str) -> String,
) -> std::borrow::Cow<'_, str> {
    if !markdown.contains(DATABASE_EMBED) {
        return markdown.into();
    }
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(start) = rest.find(DATABASE_EMBED) {
        out.push_str(&rest[..start]);
        let after = &rest[start + DATABASE_EMBED.len()..];
        match after.find("]]").map(|end| (&after[..end], end)) {
            Some((body, end)) if database_embed(body).is_some() => {
                out.push_str(&replace(body));
                rest = &after[end + 2..];
            }
            _ => {
                out.push_str(DATABASE_EMBED);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.into()
}
/// Restores the original syntax where markers must stay literal (code).
fn restore(text: &str) -> String {
    let text = text.replace(TASKS_SENTINEL, crate::journal::UNFINISHED_TASKS);
    if !text.contains(DB_MARK) {
        return text;
    }
    text.split(DB_MARK)
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 {
                format!("{DATABASE_EMBED}{part}]]")
            } else {
                part.to_string()
            }
        })
        .collect()
}
/// Members get a slot the app fills with the live database; everyone else gets
/// a short note, since the database may be private to the workspace.
fn database_slot(body: &str, links: Links) -> String {
    let Some((id, view)) = database_embed(body) else {
        return String::new();
    };
    match links {
        Links::Members => format!("<span class=\"db-embed db-id-{id} db-view-{view}\"></span>"),
        _ => {
            "<span class=\"db-embed-note\">▦ Embedded database · shown to workspace members</span>"
                .into()
        }
    }
}
fn to_html(markdown: &str, links: Links) -> String {
    let markdown = &markdown.replace(crate::journal::UNFINISHED_TASKS, TASKS_SENTINEL);
    let markdown = &*replace_database_embeds(markdown, |body| format!("{DB_MARK}{body}{DB_MARK}"));
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
    let mut events = Vec::new();
    let mut code: Option<(String, String)> = None;
    // Note links are private IDs: guests and the public see their text only.
    let mut dropped_links = 0usize;
    // Inside an image written as an audio player: its alt text is not shown.
    let mut in_audio = false;
    for event in Parser::new_ext(markdown, pulldown_cmark::Options::all()) {
        if in_audio {
            in_audio = !matches!(event, Event::End(TagEnd::Image));
            continue;
        }
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((language, String::new()));
            }
            Event::Text(text) if code.is_some() => {
                code.as_mut().unwrap().1.push_str(&restore(&text))
            }
            Event::Code(text) if text.contains(TASKS_SENTINEL) || text.contains(DB_MARK) => {
                events.push(Event::Code(restore(&text).into()))
            }
            Event::Text(text) if text.contains(DB_MARK) => {
                for (i, part) in text.split(DB_MARK).enumerate() {
                    if i % 2 == 1 {
                        events.push(Event::InlineHtml(database_slot(part, links).into()));
                    } else {
                        decorate(part, &mut events);
                    }
                }
            }
            // The live task list is filled in by the app for signed-in readers.
            Event::Text(text) if text.contains(TASKS_SENTINEL) => {
                for (i, part) in text.split(TASKS_SENTINEL).enumerate() {
                    if i > 0 && matches!(links, Links::Members | Links::Shared) {
                        events.push(Event::InlineHtml(
                            "<span class=\"journal-tasks\"></span>".into(),
                        ));
                    }
                    decorate(part, &mut events);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let (language, text) = code.take().unwrap_or_default();
                events.push(Event::Html(code_block(&language, &text).into()));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = links
                    .map(&dest_url, true)
                    .map(Into::into)
                    .unwrap_or(dest_url);
                // `![name](song.mp3)`, or an uploaded recording marked "audio".
                if &*title == "audio" || is_audio(&dest_url) {
                    events.push(Event::InlineHtml(
                        format!(
                            "<audio class=\"audio-player\" controls preload=\"metadata\" src=\"{}\"></audio>",
                            escape(&dest_url)
                        )
                        .into(),
                    ));
                    in_audio = true;
                    continue;
                }
                events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            Event::Start(Tag::Link { ref dest_url, .. })
                if !matches!(links, Links::Members) && dest_url.starts_with("#note/") =>
            {
                dropped_links += 1;
            }
            Event::End(TagEnd::Link) if dropped_links > 0 => dropped_links -= 1,
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = links
                    .map(&dest_url, false)
                    .map(Into::into)
                    .unwrap_or(dest_url);
                events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            // Members get interactive task toggles; published pages show the state.
            Event::TaskListMarker(done) if matches!(links, Links::Public(_)) => {
                events.push(Event::Text(if done { "☑ " } else { "☐ " }.into()))
            }
            Event::TaskListMarker(done) if matches!(links, Links::Export) => {
                events.push(Event::InlineHtml(
                    format!(
                        "<span class=\"task-marker\">{}</span>",
                        if done { "☑" } else { "☐" }
                    )
                    .into(),
                ))
            }
            Event::Text(text) => decorate(&text, &mut events),
            Event::InlineMath(tex) => events.push(Event::InlineHtml(math(&tex, false).into())),
            Event::DisplayMath(tex) => events.push(Event::Html(math(&tex, true).into())),
            other => events.push(other),
        }
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    html
}

/// Addresses of audio files and playlists, by their extension.
pub(crate) fn is_audio(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        ".mp3", ".wav", ".ogg", ".oga", ".opus", ".m4a", ".aac", ".flac", ".weba", ".m3u", ".m3u8",
    ]
    .iter()
    .any(|ext| path.ends_with(ext))
}
/// Splits text into `==highlights==` and inline `#tags`.
fn decorate(text: &str, events: &mut Vec<pulldown_cmark::Event<'_>>) {
    use pulldown_cmark::Event;
    let mut rest = text;
    while let Some((before, marked, after)) = highlight_split(rest) {
        tags_into(before, events);
        events.push(Event::InlineHtml("<mark>".into()));
        tags_into(marked, events);
        events.push(Event::InlineHtml("</mark>".into()));
        rest = after;
    }
    tags_into(rest, events);
}
/// Finds the first `==text==` whose content does not start or end with whitespace.
fn highlight_split(text: &str) -> Option<(&str, &str, &str)> {
    let mut from = 0;
    while let Some(open) = text[from..].find("==").map(|i| i + from) {
        let inner = open + 2;
        if let Some(close) = text[inner..].find("==").map(|i| i + inner) {
            let marked = &text[inner..close];
            if !marked.is_empty()
                && !marked.starts_with(char::is_whitespace)
                && !marked.ends_with(char::is_whitespace)
            {
                return Some((&text[..open], marked, &text[close + 2..]));
            }
        }
        from = inner;
    }
    None
}
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}
/// Inline tags: `#word` at the start of text or after whitespace or `(`, made of
/// letters, digits, `_`, `-`, `/`, with at least one letter, up to 40 characters.
fn tag_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    let mut previous: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '#' && previous.is_none_or(|p| p.is_whitespace() || p == '(') {
            let start = i + 1;
            let end = text[start..]
                .find(|c: char| !is_tag_char(c))
                .map_or(text.len(), |n| start + n);
            let name = text[start..end].trim_end_matches(['-', '/']);
            let end = start + name.len();
            if !name.is_empty()
                && name.chars().count() <= 40
                && name.chars().any(char::is_alphabetic)
            {
                spans.push((i, end));
                while chars.peek().is_some_and(|(j, _)| *j < end) {
                    chars.next();
                }
                previous = text[..end].chars().next_back();
                continue;
            }
        }
        previous = Some(c);
    }
    spans
}
/// Assignments (`@person@example.com`) and reminders (`⏰2026-10-27 09:00`).
fn chip_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    for (i, c) in text.char_indices() {
        let after_space = text[..i]
            .chars()
            .next_back()
            .is_none_or(|p| p.is_whitespace() || p == '(');
        if c == '@' && after_space {
            let len = crate::mail::mention_len(&text[i + 1..]);
            if len > 0 {
                spans.push((i, i + 1 + len));
            }
        }
        if c == '⏰' {
            let start = i + c.len_utf8();
            let stamp = text[start..].get(..16).unwrap_or("");
            let shape = stamp.bytes().enumerate().all(|(j, b)| match j {
                4 | 7 => b == b'-',
                10 => b == b' ' || b == b'T',
                13 => b == b':',
                _ => b.is_ascii_digit(),
            });
            if stamp.len() == 16 && shape {
                spans.push((i, start + 16));
            }
        }
    }
    spans
}
fn tags_into(text: &str, events: &mut Vec<pulldown_cmark::Event<'_>>) {
    use pulldown_cmark::Event;
    let mut spans: Vec<(usize, usize, bool)> = tag_spans(text)
        .into_iter()
        .map(|(a, b)| (a, b, true))
        .chain(chip_spans(text).into_iter().map(|(a, b)| (a, b, false)))
        .collect();
    spans.sort();
    let mut last = 0;
    for (start, end, tag) in spans {
        if start < last {
            continue;
        }
        if start > last {
            events.push(Event::Text(text[last..start].to_string().into()));
        }
        let piece = &text[start..end];
        let html = if tag {
            // `data-tag` lets stylesheets style particular tags, such as severities.
            format!(
                "<span class=\"tag\" data-tag=\"{}\">{}</span>",
                escape(&piece.trim_start_matches('#').to_lowercase()),
                escape(piece)
            )
        } else if let Some(email) = piece.strip_prefix('@') {
            format!(
                "<span class=\"mention\" title=\"Assigned to {0}\">@{0}</span>",
                escape(email)
            )
        } else {
            let stamp = piece.trim_start_matches('⏰').replace('T', " ");
            format!(
                "<span class=\"reminder\" title=\"Reminder by email\">⏰ {}</span>",
                escape(&stamp)
            )
        };
        events.push(Event::InlineHtml(html.into()));
        last = end;
    }
    if last < text.len() {
        events.push(Event::Text(text[last..].to_string().into()));
    }
}
/// Normalized inline tags in Markdown text, excluding code.
pub(crate) fn inline_tags(markdown: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Parser};
    let mut tags: Vec<String> = vec![];
    let mut in_code = false;
    for event in Parser::new_ext(markdown, pulldown_cmark::Options::all()) {
        match event {
            Event::Start(pulldown_cmark::Tag::CodeBlock(_)) => in_code = true,
            Event::End(pulldown_cmark::TagEnd::CodeBlock) => in_code = false,
            Event::Text(text) if !in_code => {
                for (start, end) in tag_spans(&text) {
                    let tag = text[start + 1..end].to_lowercase();
                    if !tags.contains(&tag) {
                        tags.push(tag);
                    }
                }
            }
            _ => {}
        }
    }
    tags
}
/// Normalizes a user-entered tag; `None` when it is not a valid tag.
pub(crate) fn normalize_tag(tag: &str) -> Option<String> {
    let tag = tag.trim().trim_start_matches('#').to_lowercase();
    (!tag.is_empty()
        && tag.chars().count() <= 40
        && tag.chars().all(is_tag_char)
        && tag.chars().any(char::is_alphabetic))
    .then_some(tag)
}

/// TeX to MathML Core. Unknown commands render as visible errors; nothing executes.
fn math(tex: &str, display: bool) -> String {
    use math_core::{LatexToMathML, MathCoreConfig, MathDisplay};
    static CONVERTER: OnceLock<Option<LatexToMathML>> = OnceLock::new();
    let converter = CONVERTER.get_or_init(|| {
        LatexToMathML::new(
            MathCoreConfig {
                ignore_unknown_commands: true,
                ..Default::default()
            },
            vec![],
        )
        .ok()
    });
    let mode = if display {
        MathDisplay::Block
    } else {
        MathDisplay::Inline
    };
    match converter
        .as_ref()
        .filter(|_| tex.len() <= 5_000)
        .map(|c| c.convert_with_local_state(tex, mode))
    {
        Some(Ok(result)) => result.mathml,
        _ => format!("<code class=\"math-error\">{}</code>", escape(tex)),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The first word of a fence's info string, limited to safe class characters.
fn language_token(info: &str) -> String {
    info.split(|c: char| c.is_whitespace() || c == ',' || c == '{')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '_' | '#' | '.'))
        .take(32)
        .collect::<String>()
        .to_ascii_lowercase()
}

fn code_block(info: &str, text: &str) -> String {
    let language = language_token(info);
    let class = if language.is_empty() {
        String::new()
    } else {
        format!(" class=\"language-{}\"", escape(&language))
    };
    let body = highlight(&language, text).unwrap_or_else(|| escape(text));
    let mut html = format!("<pre><code{class}>{body}</code></pre>\n");
    if let Some(problem) = diagnostic(&language, text) {
        html.push_str(&format!(
            "<p class=\"code-diagnostic\">{}</p>\n",
            escape(&problem)
        ));
    }
    html
}

/// Syntax highlighting with scope classes (`hl-*`), styled by the app and public CSS.
/// Code is only ever treated as text.
fn highlight(language: &str, text: &str) -> Option<String> {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    if language.is_empty() || text.len() > 50_000 {
        return None;
    }
    let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines);
    let token = match language {
        "js" | "jsx" | "mjs" | "ts" | "tsx" | "typescript" => "js",
        "sh" | "shell" | "zsh" | "console" => "bash",
        "yml" => "yaml",
        "c++" => "cpp",
        "py" => "python",
        "rs" => "rust",
        other => other,
    };
    let syntax = syntaxes.find_syntax_by_token(token)?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        syntaxes,
        ClassStyle::SpacedPrefixed { prefix: "hl-" },
    );
    for line in LinesWithEndings::from(text) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    Some(generator.finalize())
}

/// Lightweight diagnostics for data formats that can be checked without executing
/// anything. Currently JSON; see docs/architecture.md for the evaluation.
fn diagnostic(language: &str, text: &str) -> Option<String> {
    if language != "json" || text.trim().is_empty() {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(text)
        .err()
        .map(|e| {
            format!(
                "JSON error at line {}, column {}: {}",
                e.line(),
                e.column(),
                e
            )
        })
}

/// MathML Core elements and presentation attributes emitted by math-core. No
/// links (`href`) or event attributes.
const MATHML_TAGS: &[&str] = &[
    "math",
    "mi",
    "mn",
    "mo",
    "ms",
    "mspace",
    "mtext",
    "menclose",
    "merror",
    "mfrac",
    "mpadded",
    "mphantom",
    "mroot",
    "mrow",
    "msqrt",
    "mstyle",
    "mmultiscripts",
    "mprescripts",
    "mover",
    "msub",
    "msubsup",
    "msup",
    "munder",
    "munderover",
    "mtable",
    "mtd",
    "mtr",
    "semantics",
    "annotation",
];
const MATHML_ATTRIBUTES: &[&str] = &[
    "display",
    "displaystyle",
    "scriptlevel",
    "mathvariant",
    "lspace",
    "rspace",
    "stretchy",
    "symmetric",
    "largeop",
    "movablelimits",
    "accent",
    "accentunder",
    "form",
    "fence",
    "separator",
    "linethickness",
    "width",
    "height",
    "depth",
    "voffset",
    "columnalign",
    "rowalign",
    "columnspacing",
    "rowspacing",
    "columnlines",
    "rowlines",
    "frame",
    "notation",
    "minsize",
    "maxsize",
    "class",
    "encoding",
    "columnspan",
    "rowspan",
];
fn clean(html: &str) -> String {
    static CLEANER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    CLEANER
        .get_or_init(|| {
            let mut builder = ammonia::Builder::default();
            builder
                .add_tag_attributes("span", &["class", "data-tag", "title"])
                .add_tags(["audio"])
                .add_tag_attributes("audio", &["class", "controls", "preload", "src"])
                .add_tag_attributes("code", &["class"])
                .add_tag_attributes("p", &["class"])
                .add_tag_attributes("blockquote", &["class"])
                .add_tag_attributes("div", &["class"])
                .add_tags(MATHML_TAGS);
            for tag in MATHML_TAGS {
                builder.add_tag_attributes(tag, MATHML_ATTRIBUTES);
            }
            builder.attribute_filter(|_, attribute, value| {
                if attribute != "class" {
                    return Some(value.into());
                }
                let kept: Vec<&str> = value
                    .split_ascii_whitespace()
                    .filter(|c| {
                        c.starts_with("hl-")
                            || c.starts_with("db-")
                            || c.starts_with("language-")
                            || c.starts_with("markdown-alert-")
                            || c.starts_with("md-indent")
                            || c.starts_with("math-core-")
                            || matches!(
                                *c,
                                "code-diagnostic"
                                    | "tag"
                                    | "mention"
                                    | "reminder"
                                    | "audio-player"
                                    | "math-error"
                                    | "journal-tasks"
                                    | "task-marker"
                            )
                    })
                    .collect();
                (!kept.is_empty()).then(|| kept.join(" ").into())
            });
            builder
        })
        .clean(html)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_youtube_links_become_players() {
        assert_eq!(
            youtube(" https://www.youtube.com/watch?v=M7lc1UVf-VE&t=1m30s\n"),
            Some(("M7lc1UVf-VE".into(), 90))
        );
        assert_eq!(
            youtube("https://youtu.be/M7lc1UVf-VE?t=42"),
            Some(("M7lc1UVf-VE".into(), 42))
        );
        assert_eq!(
            youtube("https://m.youtube.com/shorts/M7lc1UVf-VE"),
            Some(("M7lc1UVf-VE".into(), 0))
        );
        for other in [
            "Watch https://youtu.be/M7lc1UVf-VE",
            "https://youtube.com.evil.test/watch?v=M7lc1UVf-VE",
            "https://user@youtube.com/watch?v=M7lc1UVf-VE",
            "https://www.youtube.com/watch?v=M7lc1UVf-VE%22%3E",
            "javascript:alert(1)",
        ] {
            assert_eq!(youtube(other), None, "{other}");
        }
        let page = render_for(
            "https://www.youtube.com/watch?v=M7lc1UVf-VE",
            Links::Public("blog/post"),
        );
        assert!(page.contains("youtube-nocookie.com/embed/M7lc1UVf-VE\""));
        assert!(page.contains("referrerpolicy=\"strict-origin-when-cross-origin\""));
        // Elsewhere the link stays Markdown (the editor draws its own player).
        assert!(!render("https://www.youtube.com/watch?v=M7lc1UVf-VE").contains("<iframe"));
    }
    #[test]
    fn highlights_known_languages_and_escapes_everything() {
        let html = render("```rust\nfn main() { let s = \"<b>\"; }\n```");
        assert!(html.contains("<code class=\"language-rust\">"));
        assert!(html.contains("hl-keyword") || html.contains("hl-storage"));
        assert!(html.contains("&lt;b&gt;"));
        assert!(!html.contains("<b>"));
        let plain = render("```unknown-lang\n<script>x</script>\n```");
        assert!(plain.contains("&lt;script&gt;"));
        let hostile = render("```\" onmouseover=\"x\n1\n```");
        assert!(!hostile.contains("onmouseover"));
        // Arbitrary classes cannot be injected through raw HTML.
        let injected = render("<span class=\"hl-keyword evil\">x</span><p class=\"admin\">y</p>");
        assert!(injected.contains("class=\"hl-keyword\""));
        assert!(!injected.contains("evil") && !injected.contains("admin"));
    }
    #[test]
    fn renders_callouts_math_highlights_and_tags() {
        let callout = render("> [!WARNING]\n> Back up first.");
        assert!(
            callout.contains("<blockquote class=\"markdown-alert-warning\">"),
            "{callout}"
        );
        let math = render("Energy $E = mc^2$ and $$\\sum_{i=1}^n i$$ and $a \\le b$");
        assert!(
            math.contains("<math") && math.contains("<msup>") && math.contains("≤"),
            "{math}"
        );
        assert!(math.contains("display=\"block\""), "{math}");
        let unknown = render("$\\href{javascript:alert(1)}{x}$");
        assert!(
            !unknown.contains("href=") && !unknown.contains("javascript:alert(1)\""),
            "{unknown}"
        );
        let hostile = render("$<script>alert(1)</script>$ <math><mi href=\"javascript:x\" onclick=\"y\">z</mi></math>");
        assert!(
            !hostile.contains("<script")
                && !hostile.contains("onclick")
                && !hostile.contains("href"),
            "{hostile}"
        );
        let marked = render("This is ==very important== but == not this == and a==b");
        assert!(marked.contains("<mark>very important</mark>"), "{marked}");
        assert_eq!(marked.matches("<mark>").count(), 1);
        let tagged =
            render("Planning #project/alpha and #ideas, not C# or #123 or `#code`\n\n# Heading");
        assert!(
            tagged.contains("<span class=\"tag\" data-tag=\"project/alpha\">#project/alpha</span>"),
            "{tagged}"
        );
        assert!(tagged.contains("<span class=\"tag\" data-tag=\"ideas\">#ideas</span>,"));
        assert!(
            !tagged.contains("#123</span>")
                && !tagged.contains("C<span")
                && !tagged.contains("#code</span>")
        );
        assert!(tagged.contains("<h1>Heading</h1>"));
        assert_eq!(
            inline_tags("#Alpha #beta `#nope`\n```\n#code\n```\n#alpha"),
            vec!["alpha", "beta"]
        );
        assert_eq!(normalize_tag(" #Work-Notes "), Some("work-notes".into()));
        assert_eq!(normalize_tag("#123"), None);
        assert_eq!(normalize_tag("two words"), None);
    }
    #[test]
    fn unfinished_tasks_token_becomes_a_placeholder() {
        let token = crate::journal::UNFINISHED_TASKS;
        assert!(render(&format!("Open items: {token}"))
            .contains("Open items: <span class=\"journal-tasks\"></span>"));
        assert!(!render_for(token, Links::Public("p")).contains("journal-tasks"));
        assert!(render(&format!("`{token}`")).contains(&format!("<code>{token}</code>")));
        assert!(render(&format!("```\n{token}\n```")).contains(token));
    }
    #[test]
    fn indented_blocks_nest_without_becoming_code() {
        assert_eq!(indentation("- item"), (0, "- item".into()));
        assert_eq!(
            indentation("  - child\n    more"),
            (1, "- child\n  more".into())
        );
        let deep = render("    Deep paragraph");
        assert!(
            deep.contains("<div class=\"md-indent md-indent-2\"><p>Deep paragraph</p>"),
            "{deep}"
        );
        assert!(!deep.contains("<pre>"));
        let fenced = render("  ```rust\n  let x = 1;\n  ```");
        assert!(
            fenced.contains("md-indent-1")
                && fenced.contains("<pre><code class=\"language-rust\">"),
            "{fenced}"
        );
        assert_eq!(
            indentation(&" ".repeat(40)).0,
            0,
            "blank blocks stay unindented"
        );
        assert_eq!(
            indentation(&format!("{}x", " ".repeat(30))).0,
            6,
            "capped at six levels"
        );
    }
    #[test]
    fn note_links_are_kept_for_members_only() {
        let md = "See [Roadmap](#note/0b8f5f9e-0000-4000-8000-000000000000) for details.";
        assert!(render(md).contains("href=\"#note/0b8f5f9e-0000-4000-8000-000000000000\""));
        for links in [Links::Shared, Links::Public("p")] {
            let html = render_for(md, links);
            assert!(
                html.contains("See Roadmap for details.") && !html.contains("href"),
                "{html}"
            );
        }
    }
    #[test]
    fn reports_json_errors_without_executing_code() {
        assert!(render("```json\n{\"a\": 1}\n```")
            .find("code-diagnostic")
            .is_none());
        let bad = render("```json\n{\"a\": 1,,}\n```");
        assert!(bad.contains("class=\"code-diagnostic\""));
        assert!(bad.contains("line 1"));
    }
    #[test]
    fn maps_attachment_links_for_each_audience() {
        let id = Uuid::new_v4();
        let md = format!("![a](/api/v1/attachments/{id})\n\n[f](/api/v1/attachments/{id})\n\n[x](https://example.com/a)");
        let members = render_for(&md, Links::Members);
        assert!(members.contains(&format!("src=\"/api/v1/attachments/{id}/preview\"")));
        assert!(members.contains(&format!("href=\"/api/v1/attachments/{id}\"")));
        let shared = render_for(&md, Links::Shared);
        assert!(shared.contains(&format!("src=\"/api/v1/shared/attachments/{id}/preview\"")));
        assert!(shared.contains(&format!("href=\"/api/v1/shared/attachments/{id}\"")));
        let public = render_for(&md, Links::Public("p/my-post"));
        assert!(public.contains(&format!("src=\"/p/my-post/files/{id}/preview\"")));
        assert!(public.contains(&format!("href=\"/p/my-post/files/{id}\"")));
        assert!(public.contains("href=\"https://example.com/a\""));
        assert!(render_for("- [x] done\n- [ ] open", Links::Public("p")).contains("☑ done"));
        assert!(!render("- [x] done").contains('☑'));
    }
}
#[cfg(test)]
mod mention_tests {
    use super::{render_for, Links};
    #[test]
    fn public_pages_hide_addresses() {
        let members = render_for("Ask @ana@test.it today", Links::Members);
        assert!(members.contains("ana@test.it"), "{members}");
        let public = render_for("Ask @ana@test.it today", Links::Public("p/x"));
        assert!(
            !public.contains("test.it") && public.contains(">@ana</span>"),
            "{public}"
        );
    }
}
#[cfg(test)]
mod audio_tests {
    #[test]
    fn renders_audio_players() {
        let html = super::render("![Song](https://example.com/a/song.MP3?x=1)");
        assert!(html.contains(r#"<audio class="audio-player" controls="" preload="metadata" src="https://example.com/a/song.MP3?x=1"></audio>"#) || html.contains(r#"<audio class="audio-player" controls preload="metadata""#), "{html}");
        assert!(!html.contains("Song"), "{html}");
        let html = super::render(
            r#"![Recording.webm](/api/v1/attachments/00000000-0000-0000-0000-000000000000/preview "audio")"#,
        );
        assert!(html.contains("<audio"), "{html}");
        let html = super::render("![Photo](https://example.com/cat.png)");
        assert!(html.contains("<img"), "{html}");
        let html = super::render("<audio src=x onplay=alert(1)></audio>");
        assert!(!html.contains("onplay"), "{html}");
    }
}
#[cfg(test)]
mod chip_tests {
    #[test]
    fn renders_mentions_and_reminders() {
        let html = super::render("Call @ana@test.it ⏰2026-10-27 09:00 #work");
        assert!(
            html.contains(
                r#"<span class="mention" title="Assigned to ana@test.it">@ana@test.it</span>"#
            ),
            "{html}"
        );
        assert!(
            html.contains(
                r#"<span class="reminder" title="Reminder by email">⏰ 2026-10-27 09:00</span>"#
            ),
            "{html}"
        );
        assert!(html.contains(r#"class="tag""#), "{html}");
    }
}
