//! Reads Anki decks: packages (`.apkg`, `.colpkg`) and plain-text exports.
//!
//! A package is a ZIP holding an SQLite collection: `collection.anki21b`
//! (zstd-compressed, Anki 2.1.50 and later), `collection.anki21`, or
//! `collection.anki2`. Newer packages also carry a legacy `collection.anki2`
//! that only says "please update Anki", so the newest one present is read.
//! Each card is rendered from its note type's templates and turned into plain
//! text, because Minerva cards are plain text. Media files are not imported:
//! pictures and sounds become `[image]` and `[audio]`.
use super::*;
use std::io::{Cursor, Read};

/// The largest collection database read, after decompression.
const COLLECTION_LIMIT: u64 = 256 * 1024 * 1024;
/// Cards read from one file, across all its decks.
const CARD_LIMIT: usize = 20_000;

pub(super) struct ImportedDeck {
    pub name: String,
    pub cards: Vec<ImportedCard>,
}
pub(super) struct ImportedCard {
    pub front: String,
    pub back: String,
    /// Scheduling carried over from Anki: Unix time, seconds, counts.
    pub due: u64,
    pub interval: u64,
    pub reviews: u32,
    pub lapses: u32,
    pub suspended: bool,
}
pub(super) struct Import {
    pub decks: Vec<ImportedDeck>,
    pub skipped: usize,
    pub warnings: Vec<String>,
    /// Pictures the cards show, by file name: (media type, bytes).
    pub media: HashMap<String, (&'static str, Vec<u8>)>,
}

/// Reads an Anki package or text export. `keep_progress` carries over review
/// history and due dates; otherwise every card starts as new.
pub(super) fn read(bytes: &[u8], file_name: &str, keep_progress: bool) -> Result<Import, ApiError> {
    if bytes.starts_with(b"PK\x03\x04") {
        package(bytes, keep_progress)
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| bad("This is not an Anki package (.apkg) or a UTF-8 text export"))?;
        Ok(text_export(text, file_name))
    }
}

fn package(bytes: &[u8], keep_progress: bool) -> Result<Import, ApiError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| bad("The Anki package could not be opened"))?;
    let names = [
        "collection.anki21b",
        "collection.anki21",
        "collection.anki2",
    ];
    let name = names
        .into_iter()
        .find(|n| zip.by_name(n).is_ok())
        .ok_or_else(|| bad("The package contains no Anki collection"))?;
    let mut raw = Vec::new();
    zip.by_name(name)
        .map_err(|_| bad("The Anki package could not be opened"))?
        .take(COLLECTION_LIMIT + 1)
        .read_to_end(&mut raw)
        .map_err(|_| bad("The Anki package is damaged"))?;
    let database = if name.ends_with('b') {
        let mut out = Vec::new();
        ruzstd::decoding::StreamingDecoder::new(&raw[..])
            .map_err(|_| bad("The Anki package is damaged"))?
            .take(COLLECTION_LIMIT + 1)
            .read_to_end(&mut out)
            .map_err(|_| bad("The Anki package is damaged"))?;
        out
    } else {
        raw
    };
    if database.len() as u64 > COLLECTION_LIMIT {
        return Err(bad("The Anki collection is too large to import"));
    }
    // SQLite reads a database from a file; it is removed when done.
    let file = TempFile(std::env::temp_dir().join(format!("minerva-anki-{}.db", id())));
    std::fs::write(&file.0, &database).map_err(|_| internal())?;
    let db = Connection::open_with_flags(&file.0, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| bad("The Anki collection could not be read"))?;
    let mut import = collection(&db, keep_progress).map_err(|e| match e {
        Some(error) => error,
        None => bad(
            "The Anki collection could not be read; it may come from an unsupported Anki version",
        ),
    })?;
    import.media = media(&mut zip, &import);
    let missing = import.decks.iter().flat_map(|d| &d.cards).any(|c| {
        pictures(&c.front)
            .chain(pictures(&c.back))
            .any(|n| !import.media.contains_key(n))
    });
    if missing {
        import.warnings.push("Some pictures were not in the package; they show as [image]. Export with media included to keep them.".into());
    }
    Ok(import)
}
/// File names of the pictures a card's text refers to.
pub(super) fn pictures(text: &str) -> impl Iterator<Item = &str> {
    text.split(IMAGE_START)
        .skip(1)
        .filter_map(|s| s.split(IMAGE_END).next())
}
const PICTURE_LIMIT: usize = 10 * 1024 * 1024;
const MEDIA_LIMIT: usize = 100 * 1024 * 1024;
fn zstd(bytes: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    ruzstd::decoding::StreamingDecoder::new(bytes)
        .ok()?
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .ok()?;
    (out.len() <= limit).then_some(out)
}
/// The picture's media type, recognised by its first bytes; other files are
/// not kept.
fn picture_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}
/// The pictures the imported cards use. The package's `media` file maps zip
/// entries ("0", "1", …) to file names: JSON in older packages, a compressed
/// protocol buffer (repeated entries of name = 1) since Anki 2.1.50, whose
/// files are compressed too.
fn media(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    import: &Import,
) -> HashMap<String, (&'static str, Vec<u8>)> {
    let wanted: HashSet<&str> = import
        .decks
        .iter()
        .flat_map(|d| &d.cards)
        .flat_map(|c| pictures(&c.front).chain(pictures(&c.back)))
        .collect();
    let mut found = HashMap::new();
    if wanted.is_empty() {
        return found;
    }
    let mut raw = Vec::new();
    let Ok(entry) = zip.by_name("media") else {
        return found;
    };
    if entry.take(16 * 1024 * 1024).read_to_end(&mut raw).is_err() {
        return found;
    }
    let mut names: Vec<(String, String)> = vec![];
    if raw.first() == Some(&b'{') {
        if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&raw) {
            names = map
                .into_iter()
                .filter_map(|(entry, name)| Some((entry, name.as_str()?.to_string())))
                .collect();
        }
    } else if let Some(list) = zstd(&raw, 16 * 1024 * 1024) {
        names = proto_messages(&list, 1)
            .into_iter()
            .enumerate()
            .filter_map(|(i, entry)| Some((i.to_string(), proto_strings(entry).remove(&1)?)))
            .collect();
    }
    let mut total = 0;
    for (entry, name) in names {
        if !wanted.contains(name.as_str()) || found.contains_key(&name) || found.len() >= 2000 {
            continue;
        }
        let mut bytes = Vec::new();
        let Ok(file) = zip.by_name(&entry) else {
            continue;
        };
        if file
            .take(PICTURE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            match zstd(&bytes, PICTURE_LIMIT) {
                Some(plain) => bytes = plain,
                None => continue,
            }
        }
        let Some(kind) = picture_type(&bytes) else {
            continue;
        };
        if bytes.len() > PICTURE_LIMIT || total + bytes.len() > MEDIA_LIMIT {
            continue;
        }
        total += bytes.len();
        found.insert(name, (kind, bytes));
    }
    found
}
struct TempFile(std::path::PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A note type: its field names in order, and its card templates by ordinal.
struct NoteType {
    fields: Vec<String>,
    templates: HashMap<i64, (String, String)>,
    cloze: bool,
}

/// `None` errors are unexpected database contents.
fn collection(db: &Connection, keep_progress: bool) -> Result<Import, Option<ApiError>> {
    let sql = |e: rusqlite::Error| {
        let _ = e;
        None
    };
    let table = |name: &str| -> bool {
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [name],
            |r| r.get(0),
        )
        .unwrap_or(false)
    };
    let created: i64 = db
        .query_row("SELECT crt FROM col", [], |r| r.get(0))
        .map_err(sql)?;
    let mut types: HashMap<i64, NoteType> = HashMap::new();
    let mut decks: HashMap<i64, String> = HashMap::new();
    if table("notetypes") {
        // Anki 2.1.28 and later: note types, fields, and templates in tables.
        let mut stmt = db
            .prepare("SELECT ntid, name FROM fields ORDER BY ntid, ord")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?;
        for row in rows {
            let (ntid, name) = row.map_err(sql)?;
            types
                .entry(ntid)
                .or_insert_with(|| NoteType {
                    fields: vec![],
                    templates: HashMap::new(),
                    cloze: false,
                })
                .fields
                .push(name);
        }
        let mut stmt = db
            .prepare("SELECT ntid, ord, config FROM templates")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(sql)?;
        for row in rows {
            let (ntid, ord, config) = row.map_err(sql)?;
            // Template config is a protocol buffer: 1 = question, 2 = answer.
            let strings = proto_strings(&config);
            let question = strings.get(&1).cloned().unwrap_or_default();
            let answer = strings.get(&2).cloned().unwrap_or_default();
            if let Some(t) = types.get_mut(&ntid) {
                t.cloze |= question.contains("cloze:");
                t.templates.insert(ord, (question, answer));
            }
        }
    } else {
        let models: String = db
            .query_row("SELECT models FROM col", [], |r| r.get(0))
            .map_err(sql)?;
        let models: Value = serde_json::from_str(&models).map_err(|_| None)?;
        for (id, m) in models.as_object().into_iter().flatten() {
            let Ok(id) = id.parse::<i64>() else { continue };
            let mut fields: Vec<(i64, String)> = m["flds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|f| {
                    (
                        f["ord"].as_i64().unwrap_or(0),
                        f["name"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect();
            fields.sort_by_key(|f| f.0);
            let templates = m["tmpls"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| {
                    (
                        t["ord"].as_i64().unwrap_or(0),
                        (
                            t["qfmt"].as_str().unwrap_or("").to_string(),
                            t["afmt"].as_str().unwrap_or("").to_string(),
                        ),
                    )
                })
                .collect();
            types.insert(
                id,
                NoteType {
                    fields: fields.into_iter().map(|f| f.1).collect(),
                    templates,
                    cloze: m["type"].as_i64() == Some(1),
                },
            );
        }
    }
    if table("decks") {
        let mut stmt = db.prepare("SELECT id, name FROM decks").map_err(sql)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql)?;
        for row in rows {
            let (id, name) = row.map_err(sql)?;
            decks.insert(id, name);
        }
    } else {
        let raw: String = db
            .query_row("SELECT decks FROM col", [], |r| r.get(0))
            .map_err(sql)?;
        let all: Value = serde_json::from_str(&raw).map_err(|_| None)?;
        for (id, d) in all.as_object().into_iter().flatten() {
            if let (Ok(id), Some(name)) = (id.parse(), d["name"].as_str()) {
                decks.insert(id, name.to_string());
            }
        }
    }

    // Cards in a filtered deck count for their original deck.
    let mut stmt = db
        .prepare(
            "SELECT CASE WHEN c.odid != 0 THEN c.odid ELSE c.did END, c.ord, c.type, c.queue,
                    c.due, c.ivl, c.reps, c.lapses, n.mid, n.flds
             FROM cards c JOIN notes n ON n.id = c.nid
             ORDER BY 1, n.id, c.ord",
        )
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, String>(9)?,
            ))
        })
        .map_err(sql)?;
    let mut by_deck: Vec<(i64, ImportedDeck)> = vec![];
    let (mut skipped, mut read) = (0, 0);
    let mut warnings = vec![];
    for row in rows {
        let (deck, ord, kind, queue, due, ivl, reps, lapses, mid, flds) = row.map_err(sql)?;
        read += 1;
        if read > CARD_LIMIT {
            warnings.push(format!("Only the first {CARD_LIMIT} cards were read."));
            break;
        }
        let Some(note_type) = types.get(&mid) else {
            skipped += 1;
            continue;
        };
        let values: Vec<&str> = flds.split('\u{1f}').collect();
        let Some((front, back)) = render_card(note_type, &values, ord) else {
            skipped += 1;
            continue;
        };
        let mut card = ImportedCard {
            front,
            back,
            due: now(),
            interval: 0,
            reviews: 0,
            lapses: 0,
            suspended: false,
        };
        if keep_progress {
            schedule(&mut card, created, kind, queue, due, ivl, reps, lapses);
        }
        let name = decks
            .get(&deck)
            .map(|n| n.replace(['\u{1f}'], "::"))
            .unwrap_or_else(|| "Anki".into());
        match by_deck.iter_mut().find(|(id, _)| *id == deck) {
            Some((_, d)) => d.cards.push(card),
            None => by_deck.push((
                deck,
                ImportedDeck {
                    name,
                    cards: vec![card],
                },
            )),
        }
    }
    Ok(Import {
        decks: by_deck.into_iter().map(|(_, d)| d).collect(),
        skipped,
        warnings,
        media: HashMap::new(),
    })
}

/// Anki stores review due dates as days since the collection was created and
/// learning steps as Unix times; intervals are days.
#[allow(clippy::too_many_arguments)]
fn schedule(
    card: &mut ImportedCard,
    created: i64,
    kind: i64,
    queue: i64,
    due: i64,
    ivl: i64,
    reps: i64,
    lapses: i64,
) {
    let day = 86400;
    card.reviews = reps.clamp(0, u32::MAX as i64) as u32;
    card.lapses = lapses.clamp(0, u32::MAX as i64) as u32;
    card.suspended = queue == -1;
    let at = |seconds: i64| seconds.max(0) as u64;
    match kind {
        // Learning: due is a Unix time; the step is short.
        1 => {
            card.interval = 600;
            card.due = if queue == 1 {
                at(due)
            } else {
                at(created + due * day)
            };
        }
        // Review and relearning.
        2 | 3 => {
            card.interval = (ivl.max(1) as u64 * day as u64).min(365 * day as u64);
            card.due = if queue == 1 {
                at(due)
            } else {
                at(created + due * day)
            };
        }
        // New: due now.
        _ => {}
    }
}

/// The question and answer of card `ord` of a note, as plain text.
fn render_card(t: &NoteType, values: &[&str], ord: i64) -> Option<(String, String)> {
    let fields: HashMap<&str, &str> = t
        .fields
        .iter()
        .map(String::as_str)
        .zip(values.iter().copied().chain(std::iter::repeat("")))
        .collect();
    // Cloze notes have one template; card ordinals are the cloze numbers - 1.
    let (template, cloze) = if t.cloze {
        (t.templates.values().next()?, Some(ord + 1))
    } else {
        (t.templates.get(&ord)?, None)
    };
    let question = render(&template.0, &fields, cloze, true, "");
    // The answer usually repeats the question above `<hr id=answer>`.
    let mut answer = render(&template.1, &fields, cloze, false, "");
    if let Some(i) = answer
        .find("<hr id=answer>")
        .or_else(|| answer.find("<hr id=\"answer\">"))
    {
        answer = answer[i..].split_once('>').map_or("", |x| x.1).to_string();
    }
    let front = limit(&text(&question), 4000);
    let back = limit(&text(&answer), 8000);
    if front.is_empty() {
        return None;
    }
    Some((front, if back.is_empty() { "—".into() } else { back }))
}
pub(super) fn limit(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.into();
    }
    let mut end = max - 3;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// A small subset of Anki's template language: fields, filters, sections,
/// `{{FrontSide}}`, and cloze deletions.
fn render(
    template: &str,
    fields: &HashMap<&str, &str>,
    cloze: Option<i64>,
    question: bool,
    front: &str,
) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find("}}") else {
            rest = &rest[start..];
            break;
        };
        let tag = rest[start + 2..start + end].trim();
        rest = &rest[start + end + 2..];
        if let Some(name) = tag.strip_prefix('#').or_else(|| tag.strip_prefix('^')) {
            let close = format!("{{{{/{}}}}}", name.trim());
            let (inner, after) = match rest.find(&close) {
                Some(i) => (&rest[..i], &rest[i + close.len()..]),
                None => (rest, ""),
            };
            let filled = fields
                .get(name.trim())
                .is_some_and(|v| !text(v).trim().is_empty());
            if filled == tag.starts_with('#') {
                out.push_str(&render(inner, fields, cloze, question, front));
            }
            rest = after;
            continue;
        }
        if tag.starts_with('/') {
            continue;
        }
        if tag == "FrontSide" {
            out.push_str(front);
            continue;
        }
        // `filter:filter:Field`; `tts lang:Field` is a filter with options.
        let mut parts: Vec<&str> = tag.split(':').collect();
        let name = parts.pop().unwrap_or("").trim();
        let value = fields.get(name).copied().unwrap_or("");
        let filters: Vec<&str> = parts
            .iter()
            .map(|f| f.split_whitespace().next().unwrap_or(""))
            .collect();
        if filters.contains(&"type") {
            continue; // A typing box has no plain-text equivalent.
        }
        if filters.contains(&"cloze") {
            out.push_str(&cloze_text(value, cloze.unwrap_or(0), question));
        } else {
            out.push_str(value);
        }
    }
    out.push_str(rest);
    out
}
/// `{{c1::answer::hint}}`: on the question, the current deletion becomes
/// `[...]` (or `[hint]`); everywhere else its answer shows.
fn cloze_text(value: &str, current: i64, question: bool) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("{{c") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        let (Ok(number), Some(body)) = (
            digits.parse::<i64>(),
            after[digits.len()..].strip_prefix("::"),
        ) else {
            out.push_str("{{c");
            rest = after;
            continue;
        };
        let Some(end) = body.find("}}") else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &body[..end];
        let (answer, hint) = match inner.split_once("::") {
            Some((a, h)) => (a, Some(h)),
            None => (inner, None),
        };
        if question && number == current {
            out.push_str(&format!("[{}]", hint.unwrap_or("...")));
        } else {
            out.push_str(answer);
        }
        rest = &body[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Marks a picture from the package's media, by file name, until the import
/// stores it: `\u{e000}name\u{e001}`.
pub(super) const IMAGE_START: char = '\u{e000}';
pub(super) const IMAGE_END: char = '\u{e001}';

/// The value of an HTML attribute in a tag's source, entities decoded.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before = lower[..at].chars().last();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            return Some(String::new());
        };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or(""),
        };
        return Some(decode_entities(value));
    }
    None
}
fn web_url(url: &str) -> bool {
    url::Url::parse(url)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        && !url.contains([' ', '(', ')', '<', '>', '"'])
}
/// Anki fields are HTML; Minerva cards are text with links (`[text](url)`)
/// and pictures. What a card's styles or scripts hide is left out, such as a
/// template's hidden copy of the cloze text or its buttons.
fn text(html: &str) -> String {
    const VOID: [&str; 8] = ["br", "hr", "img", "input", "meta", "link", "source", "wbr"];
    let mut out = String::new();
    let mut rest = html;
    // An element being left out with everything inside it: (name, depth).
    let mut skip: Option<(String, usize)> = None;
    let mut links: Vec<Option<String>> = vec![];
    while let Some(start) = rest.find('<') {
        if skip.is_none() {
            out.push_str(&rest[..start]);
        }
        let Some(end) = rest[start..].find('>') else {
            if skip.is_none() {
                out.push_str(&rest[start..]);
            }
            rest = "";
            break;
        };
        let source = &rest[start + 1..start + end];
        let tag = source.trim().to_ascii_lowercase();
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        rest = &rest[start + end + 1..];
        if name.is_empty() {
            continue; // Comments and other markup.
        }
        let void = VOID.contains(&name.as_str()) || tag.ends_with('/');
        if let Some((skipped, depth)) = &mut skip {
            if *skipped == name && !void {
                if closing {
                    *depth -= 1;
                    if *depth == 0 {
                        skip = None;
                    }
                } else {
                    *depth += 1;
                }
            }
            continue;
        }
        if !closing && !void {
            let style: String = attribute(source, "style")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let hidden = matches!(
                name.as_str(),
                "script"
                    | "style"
                    | "template"
                    | "noscript"
                    | "button"
                    | "select"
                    | "textarea"
                    | "head"
                    | "title"
            ) || attribute(source, "hidden").is_some()
                || style.contains("display:none")
                || style.contains("visibility:hidden");
            if hidden {
                skip = Some((name, 1));
                continue;
            }
        }
        match name.as_str() {
            "br" | "hr" => out.push('\n'),
            // Anki starts each new line of a field with a <div>.
            "div" | "p" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
            | "table"
                if !closing && !out.is_empty() && !out.ends_with('\n') =>
            {
                out.push('\n')
            }
            "div" | "p" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
            | "table"
                if closing =>
            {
                out.push('\n')
            }
            "li" if !closing => out.push_str("\n• "),
            "td" | "th" if closing => out.push_str("  "),
            "a" if !closing => {
                let href = attribute(source, "href").filter(|h| web_url(h));
                if href.is_some() {
                    out.push('[');
                }
                links.push(href);
            }
            "a" => {
                if let Some(Some(url)) = links.pop() {
                    if out.ends_with('[') {
                        out.pop();
                        out.push_str(&url);
                    } else {
                        out.push_str(&format!("]({url})"));
                    }
                }
            }
            "img" => match attribute(source, "src").filter(|s| !s.is_empty()) {
                Some(src) if web_url(&src) => out.push_str(&format!("![]({src})")),
                Some(src)
                    if !src.contains(['/', '\\', IMAGE_START, IMAGE_END]) && !src.contains(':') =>
                {
                    out.push(IMAGE_START);
                    out.push_str(&src);
                    out.push(IMAGE_END);
                }
                _ => out.push_str("[image]"),
            },
            _ => {}
        }
    }
    if skip.is_none() {
        out.push_str(rest);
    }
    let mut text = decode_entities(&out);
    // Sounds: `[sound:file.mp3]`.
    while let Some(start) = text.find("[sound:") {
        match text[start..].find(']') {
            Some(end) => text.replace_range(start..start + end + 1, "[audio]"),
            None => break,
        }
    }
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut joined = lines.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    joined.trim().to_string()
}
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find(';').filter(|e| *e <= 10);
        let decoded = end.and_then(|end| {
            let entity = &rest[1..end];
            let c = match entity {
                "nbsp" => Some(' '),
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
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

/// Every length-delimited field `number` of a protocol buffer message.
fn proto_messages(mut buf: &[u8], number: u64) -> Vec<&[u8]> {
    let mut out = vec![];
    while let Some(key) = proto_varint(&mut buf) {
        let skip = match key & 7 {
            0 => {
                if proto_varint(&mut buf).is_none() {
                    break;
                }
                0
            }
            1 => 8,
            5 => 4,
            2 => {
                let Some(len) = proto_varint(&mut buf).and_then(|l| usize::try_from(l).ok()) else {
                    break;
                };
                if len > buf.len() {
                    break;
                }
                if key >> 3 == number {
                    out.push(&buf[..len]);
                }
                len
            }
            _ => break,
        };
        if skip > buf.len() {
            break;
        }
        buf = &buf[skip..];
    }
    out
}
fn proto_varint(buf: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = buf.split_first()?;
        *buf = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}
/// The length-delimited fields of a protocol buffer message, by number.
fn proto_strings(mut buf: &[u8]) -> HashMap<u64, String> {
    fn varint(buf: &mut &[u8]) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = buf.split_first()?;
            *buf = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
    let mut out = HashMap::new();
    while let Some(key) = varint(&mut buf) {
        let skip = match key & 7 {
            0 => {
                if varint(&mut buf).is_none() {
                    break;
                }
                0
            }
            1 => 8,
            5 => 4,
            2 => {
                let Some(len) = varint(&mut buf).and_then(|l| usize::try_from(l).ok()) else {
                    break;
                };
                if len > buf.len() {
                    break;
                }
                if let Ok(s) = std::str::from_utf8(&buf[..len]) {
                    out.entry(key >> 3).or_insert_with(|| s.to_string());
                }
                len
            }
            _ => break,
        };
        if skip > buf.len() {
            break;
        }
        buf = &buf[skip..];
    }
    out
}

/// Anki's "Notes in Plain Text" export and similar CSV files: optional
/// `#separator:`, `#html:`, and `#… column:` headers, then one note per line.
fn text_export(export: &str, file_name: &str) -> Import {
    let mut separator = None;
    let mut html = false;
    let mut meta = HashSet::new();
    let mut deck_column = None;
    let mut body = Vec::new();
    for line in export.trim_start_matches('\u{feff}').lines() {
        if body.is_empty() {
            if let Some(header) = line.strip_prefix('#') {
                if let Some((key, value)) = header.split_once(':') {
                    let value = value.trim();
                    match key.trim() {
                        "separator" => {
                            separator = Some(match value.to_ascii_lowercase().as_str() {
                                "tab" => '\t',
                                "comma" => ',',
                                "semicolon" => ';',
                                "space" => ' ',
                                "pipe" => '|',
                                "colon" => ':',
                                other => other.chars().next().unwrap_or('\t'),
                            })
                        }
                        "html" => html = value == "true",
                        key if key.ends_with(" column") => {
                            if let Ok(column) = value.parse::<usize>() {
                                if key == "deck column" {
                                    deck_column = Some(column - 1);
                                }
                                meta.insert(column.saturating_sub(1));
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }
        }
        body.push(line);
    }
    let joined = body.join("\n");
    let separator = separator.unwrap_or(if joined.contains('\t') {
        '\t'
    } else if joined.contains(';') {
        ';'
    } else {
        ','
    });
    let base = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file_name)
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let base = if base.trim().is_empty() {
        "Anki"
    } else {
        base.trim()
    };
    let mut decks: Vec<ImportedDeck> = vec![];
    let mut skipped = 0;
    let mut warnings = vec![];
    for (n, record) in csv(&joined, separator).into_iter().enumerate() {
        if n >= CARD_LIMIT {
            warnings.push(format!("Only the first {CARD_LIMIT} cards were read."));
            break;
        }
        let content: Vec<&String> = record
            .iter()
            .enumerate()
            .filter(|(i, _)| !meta.contains(i))
            .map(|(_, v)| v)
            .collect();
        let clean = |v: &str| if html { text(v) } else { v.trim().to_string() };
        let front = limit(&clean(content.first().map_or("", |v| v.as_str())), 4000);
        let back = limit(&clean(content.get(1).map_or("", |v| v.as_str())), 8000);
        if front.is_empty() {
            if record.iter().any(|v| !v.trim().is_empty()) {
                skipped += 1;
            }
            continue;
        }
        let name = deck_column
            .and_then(|c| record.get(c))
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| base.to_string());
        let card = ImportedCard {
            front,
            back: if back.is_empty() { "—".into() } else { back },
            due: now(),
            interval: 0,
            reviews: 0,
            lapses: 0,
            suspended: false,
        };
        match decks.iter_mut().find(|d| d.name == name) {
            Some(d) => d.cards.push(card),
            None => decks.push(ImportedDeck {
                name,
                cards: vec![card],
            }),
        }
    }
    Import {
        decks,
        skipped,
        warnings,
        media: HashMap::new(),
    }
}
/// Separated values with double-quoted fields, which may contain the
/// separator, newlines, and doubled quotes.
fn csv(text: &str, separator: char) -> Vec<Vec<String>> {
    let mut records = vec![];
    let mut record = vec![];
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if field.is_empty() => quoted = true,
            c if c == separator && !quoted => record.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            '\r' if !quoted => {}
            c => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates_cloze_and_html_become_plain_text() {
        let fields = HashMap::from([
            ("Front", "<b>Capital</b> of France?"),
            ("Back", "Paris&nbsp;<img src=\"x.png\">"),
            ("Extra", ""),
        ]);
        assert_eq!(
            text(&render("{{Front}}", &fields, None, true, "")),
            "Capital of France?"
        );
        assert_eq!(
            text(&render(
                "{{FrontSide}}<hr id=answer>{{Back}}{{#Extra}}<br>{{Extra}}{{/Extra}}",
                &fields,
                None,
                false,
                ""
            )),
            "Paris \u{e000}x.png\u{e001}"
        );
        // Hidden template parts and buttons are left out; links are kept.
        assert_eq!(
            text("<div style='display: none'>{{c1::raw}}<div>nested</div></div><button>Show All</button>Seen <a href=\"https://example.com/a?b=1&amp;c=2\">here</a> <a href=\"javascript:x\">no</a>"),
            "Seen [here](https://example.com/a?b=1&c=2) no"
        );
        let cloze = "{{c1::Paris}} is the capital of {{c2::France::country}}.";
        assert_eq!(
            cloze_text(cloze, 1, true),
            "[...] is the capital of France."
        );
        assert_eq!(
            cloze_text(cloze, 2, true),
            "Paris is the capital of [country]."
        );
        assert_eq!(
            cloze_text(cloze, 2, false),
            "Paris is the capital of France."
        );
        assert_eq!(
            text("a<div>b</div>[sound:x.mp3]<script>alert(1)</script>c &#x41;&lt;"),
            "a\nb\n[audio]c A<"
        );
        assert_eq!(
            limit("ééé", 5),
            "é…",
            "cut at a character boundary, within the limit"
        );
    }
    #[test]
    fn protocol_buffer_strings_are_read() {
        // Field 1 "Q", field 3 varint 7, field 2 "A".
        let buf = [0x0a, 1, b'Q', 0x18, 7, 0x12, 1, b'A'];
        let strings = proto_strings(&buf);
        assert_eq!(strings[&1], "Q");
        assert_eq!(strings[&2], "A");
        assert!(
            proto_strings(&[0x0a, 200]).is_empty(),
            "truncated input is ignored"
        );
    }
    #[test]
    fn plain_text_exports_with_headers_and_quotes() {
        let export = "#separator:tab\n#html:true\n#deck column:3\nOne\t<i>uno</i>\tSpanish\n\"Two\nlines\"\t\"dos \"\"2\"\"\"\tSpanish\nThree\ttres\tOther\n";
        let import = text_export(export, "words.txt");
        assert_eq!(import.decks.len(), 2);
        assert_eq!(import.decks[0].name, "Spanish");
        assert_eq!(import.decks[0].cards[1].front, "Two\nlines");
        assert_eq!(import.decks[0].cards[1].back, "dos \"2\"");
        assert_eq!(import.decks[0].cards[0].back, "uno");
        let plain = text_export("hola,hello\nadios,goodbye", "/x/basic.csv");
        assert_eq!(plain.decks[0].name, "basic");
        assert_eq!(plain.decks[0].cards.len(), 2);
    }
}
