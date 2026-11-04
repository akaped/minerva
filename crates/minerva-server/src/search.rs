//! Full-text search over note titles, text and tags with SQLite FTS5. Index rows
//! share their row ID with the note's row in `notes` (see `storage.rs`, which
//! keeps it stable and updates the index in the same transaction). Matching
//! ignores case and accents, and ranks title matches above tags above text.
//!
//! Database records have their own index (`record_search`, rewritten with their
//! database), and so does the text of attached PDFs and plain-text files
//! (`file_search`), which a background worker extracts after upload, outside
//! the store's lock (see `spawn`).
use super::*;

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS note_search
            USING fts5(title, body, tags, tokenize='unicode61 remove_diacritics 2');
        CREATE VIRTUAL TABLE IF NOT EXISTS record_search
            USING fts5(database_id UNINDEXED, record_id UNINDEXED, title, body,
                tokenize='unicode61 remove_diacritics 2');
        CREATE VIRTUAL TABLE IF NOT EXISTS file_search
            USING fts5(attachment_id UNINDEXED, name, body,
                tokenize='unicode61 remove_diacritics 2');
        CREATE TABLE IF NOT EXISTS file_text(attachment_id TEXT PRIMARY KEY, indexed INTEGER NOT NULL);",
    )
}

/// A cell's words: text, numbers, dates, choices; relations, people and
/// computed values are left out.
fn cell_text(c: &Column, v: &Value) -> String {
    match c.kind {
        ColumnKind::Text | ColumnKind::Url | ColumnKind::Select | ColumnKind::Date => {
            v.as_str().unwrap_or_default().to_string()
        }
        ColumnKind::Number => v.as_f64().map(|n| n.to_string()).unwrap_or_default(),
        ColumnKind::MultiSelect => v
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}
/// Rewrites the index rows of a database's records (none when it is gone).
pub(super) fn index_database(
    db: &Connection,
    id: &str,
    base: Option<&Database>,
) -> rusqlite::Result<()> {
    db.prepare_cached("DELETE FROM record_search WHERE database_id=?1")?
        .execute([id])?;
    let Some(base) = base else { return Ok(()) };
    let mut insert = db.prepare_cached(
        "INSERT INTO record_search(database_id, record_id, title, body) VALUES(?1,?2,?3,?4)",
    )?;
    for r in base.rows.iter().filter(|r| !r.archived) {
        let body: Vec<String> = base
            .columns
            .iter()
            .skip(1)
            .filter_map(|c| r.cells.get(&c.id).map(|v| cell_text(c, v)))
            .filter(|t| !t.is_empty())
            .collect();
        insert.execute(params![
            base.id,
            r.id,
            databases::record_title(base, r),
            body.join("\n")
        ])?;
    }
    Ok(())
}
pub(super) fn remove(db: &Connection, id: &str) -> rusqlite::Result<()> {
    db.prepare_cached("DELETE FROM note_search WHERE rowid=(SELECT rowid FROM notes WHERE id=?1)")?
        .execute([id])?;
    Ok(())
}
pub(super) fn index(db: &Connection, n: &Note) -> rusqlite::Result<()> {
    let tags: Vec<String> = n.tags.iter().cloned().chain(inline_tags(n)).collect();
    db.prepare_cached(
        "INSERT INTO note_search(rowid, title, body, tags) SELECT rowid, ?2, ?3, ?4 FROM notes WHERE id=?1",
    )?
    .execute(params![n.id, n.title, text(n), tags.join(" ")])?;
    Ok(())
}
/// The version of the indexed text; changing what is indexed rebuilds the index.
const INDEX_VERSION: &str = "3";
/// Rebuilds the index unless it holds one row per note, of this version.
pub(super) fn ensure(db: &Connection, d: &Data) -> rusqlite::Result<()> {
    let count: usize = db.query_row("SELECT count(*) FROM note_search", [], |r| r.get(0))?;
    let version: Option<String> = db
        .query_row("SELECT payload FROM settings WHERE key='search'", [], |r| {
            r.get(0)
        })
        .optional()?;
    let records: usize = db.query_row("SELECT count(*) FROM record_search", [], |r| r.get(0))?;
    let live = d
        .databases
        .iter()
        .map(|b| b.rows.iter().filter(|r| !r.archived).count())
        .sum::<usize>();
    if count != d.notes.len() || records != live || version.as_deref() != Some(INDEX_VERSION) {
        db.execute(
            "INSERT OR REPLACE INTO settings(key, payload) VALUES('search', ?1)",
            [INDEX_VERSION],
        )?;
        db.execute("DELETE FROM note_search", [])?;
        for n in &d.notes {
            index(db, n)?;
        }
        db.execute("DELETE FROM record_search", [])?;
        for b in &d.databases {
            index_database(db, &b.id, Some(b))?;
        }
    }
    Ok(())
}
/// Text without Minerva's own inline marks: `==highlights==` and cloze
/// deletions (`{{c1::answer::hint}}`, which keep their answer).
fn unmarked(text: &str) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (start, end, _, answer, _) in render::clozes(text) {
        out.push_str(&text[at..start]);
        out.push_str(answer);
        at = end;
    }
    out.push_str(&text[at..]);
    out.replace("==", "")
}
/// A note's words without Markdown syntax or link targets, a line per block.
fn text(n: &Note) -> String {
    use pulldown_cmark::{Event, Options, Parser, TagEnd};
    let mut out = String::new();
    for b in &n.blocks {
        for event in Parser::new_ext(&b.markdown, Options::all()) {
            match event {
                Event::Text(t) => out.push_str(&unmarked(&t)),
                Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => out.push_str(&t),
                Event::SoftBreak
                | Event::HardBreak
                | Event::End(
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::Item
                    | TagEnd::TableCell
                    | TagEnd::CodeBlock,
                ) => out.push(' '),
                _ => {}
            }
        }
        out.push('\n');
    }
    out
}
/// Every word, the last also as a prefix. Quoted words are never read as FTS5
/// operators (AND, NEAR, column filters…).
fn expression(words: &[&str]) -> String {
    format!(
        "{}*",
        words
            .iter()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" ")
    )
}
/// The words of a query, as the index splits text into words.
pub(super) fn words(query: &str) -> Vec<&str> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}
/// The first `limit` notes that callers `keep` (they check access) with every word
/// of `query`, best first, each with a snippet of its text that marks matches with
/// U+0001…U+0002. The last word also matches as a prefix, so results come while
/// typing. A query without words (an emoji, say) matches titles and text
/// containing it, without a snippet.
pub(super) fn matches<'a>(
    db: &Connection,
    d: &'a Data,
    query: &str,
    keep: impl Fn(&Note) -> bool,
    limit: usize,
) -> Result<Vec<(&'a Note, String)>, ApiError> {
    let words = words(query);
    if words.is_empty() {
        let q = query.trim().to_lowercase();
        return Ok(d
            .notes
            .iter()
            .filter(|n| {
                keep(n)
                    && (n.title.to_lowercase().contains(&q)
                        || n.blocks
                            .iter()
                            .any(|b| b.markdown.to_lowercase().contains(&q)))
            })
            .take(limit)
            .map(|n| (&**n, String::new()))
            .collect());
    }
    let expression = expression(&words);
    let by_id: HashMap<&str, &Note> = d.notes.iter().map(|n| (n.id.as_str(), &**n)).collect();
    let mut ranked = db
        .prepare_cached(
            "SELECT notes.id FROM note_search JOIN notes ON notes.rowid = note_search.rowid
            WHERE note_search MATCH ?1 ORDER BY bm25(note_search, 10.0, 1.0, 5.0)",
        )
        .map_err(|_| internal())?;
    let mut found = vec![];
    for id in ranked
        .query_map([&expression], |r| r.get::<_, String>(0))
        .map_err(|_| internal())?
    {
        let id = id.map_err(|_| internal())?;
        if let Some(n) = by_id.get(id.as_str()).filter(|n| keep(n)) {
            found.push(*n);
            if found.len() == limit {
                break;
            }
        }
    }
    // Snippets cost more than ranking: only for the notes returned.
    let mut snippet = db
        .prepare_cached(
            "SELECT snippet(note_search, 1, char(1), char(2), '…', 16) FROM note_search
            WHERE note_search MATCH ?1 AND rowid = (SELECT rowid FROM notes WHERE id = ?2)",
        )
        .map_err(|_| internal())?;
    found
        .into_iter()
        .map(|n| {
            let text = snippet
                .query_row(params![expression, n.id], |r| r.get(0))
                .optional()
                .map_err(|_| internal())?;
            Ok((n, text.unwrap_or_default()))
        })
        .collect()
}
/// Notes that callers `keep` sharing words with `text` (a question, say), most
/// related first: any word may match, and rarer words weigh more. Words shorter
/// than three letters are left out.
pub(super) fn related<'a>(
    db: &Connection,
    d: &'a Data,
    text: &str,
    keep: impl Fn(&Note) -> bool,
    limit: usize,
) -> Result<Vec<&'a Note>, ApiError> {
    let mut chosen: Vec<String> = vec![];
    for word in words(text) {
        let word = word.to_lowercase();
        if word.chars().count() >= 3 && !chosen.contains(&word) && chosen.len() < 24 {
            chosen.push(word);
        }
    }
    if chosen.is_empty() {
        return Ok(vec![]);
    }
    let expression = chosen
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let by_id: HashMap<&str, &Note> = d.notes.iter().map(|n| (n.id.as_str(), &**n)).collect();
    let mut stmt = db
        .prepare_cached(
            "SELECT notes.id FROM note_search JOIN notes ON notes.rowid = note_search.rowid
            WHERE note_search MATCH ?1 ORDER BY bm25(note_search, 10.0, 1.0, 5.0) LIMIT 2000",
        )
        .map_err(|_| internal())?;
    let mut found = vec![];
    for id in stmt
        .query_map([expression], |r| r.get::<_, String>(0))
        .map_err(|_| internal())?
    {
        let id = id.map_err(|_| internal())?;
        if let Some(n) = by_id.get(id.as_str()).filter(|n| keep(n)) {
            found.push(*n);
            if found.len() == limit {
                break;
            }
        }
    }
    Ok(found)
}

/// Records with every word of `query` in databases callers `keep`, best first,
/// with a snippet of their other properties.
pub(super) fn records<'a>(
    db: &Connection,
    d: &'a Data,
    query: &str,
    keep: impl Fn(&Database, &Record) -> bool,
    limit: usize,
) -> Result<Vec<(&'a Database, &'a Record, String)>, ApiError> {
    let words = words(query);
    if words.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = db
        .prepare_cached(
            "SELECT database_id, record_id, snippet(record_search, 3, char(1), char(2), '…', 12)
            FROM record_search WHERE record_search MATCH ?1
            ORDER BY bm25(record_search, 0.0, 0.0, 10.0, 1.0) LIMIT 400",
        )
        .map_err(|_| internal())?;
    let rows = stmt
        .query_map([expression(&words)], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })
        .map_err(|_| internal())?;
    let mut found = vec![];
    for row in rows {
        let (did, rid, snippet): (String, String, String) = row.map_err(|_| internal())?;
        let Some(base) = d.databases.iter().find(|b| b.id == did) else {
            continue;
        };
        let Some(r) = base.rows.iter().find(|r| r.id == rid && !r.archived) else {
            continue;
        };
        if keep(base, r) {
            found.push((base, r, snippet));
            if found.len() == limit {
                break;
            }
        }
    }
    Ok(found)
}

/// Attached files whose name or text has every word of `query`, on notes callers
/// `keep`: `(note, attachment ID, file name, snippet)`, best first.
pub(super) fn files<'a>(
    db: &Connection,
    d: &'a Data,
    query: &str,
    keep: impl Fn(&Note) -> bool,
    limit: usize,
) -> Result<Vec<(&'a Note, String, String, String)>, ApiError> {
    let words = words(query);
    if words.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = db
        .prepare_cached(
            "SELECT a.id, a.note_id, a.filename, snippet(file_search, 2, char(1), char(2), '…', 14)
            FROM file_search JOIN attachments a ON a.id = file_search.attachment_id
            WHERE file_search MATCH ?1 ORDER BY bm25(file_search, 0.0, 5.0, 1.0) LIMIT 400",
        )
        .map_err(|_| internal())?;
    let rows = stmt
        .query_map([expression(&words)], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| internal())?;
    let mut found = vec![];
    for row in rows {
        let (aid, nid, name, snippet) = row.map_err(|_| internal())?;
        if let Some(n) = d.notes.iter().find(|n| n.id == nid).filter(|n| keep(n)) {
            found.push((&**n, aid, name, snippet));
            if found.len() == limit {
                break;
            }
        }
    }
    Ok(found)
}

/// Text kept per file, at most.
const FILE_TEXT: usize = 400_000;

/// The words of a file: a PDF's text, or a text file's; `None` for other files.
pub(super) fn file_words(name: &str, media: &str, bytes: &[u8]) -> Option<String> {
    let lower = name.to_lowercase();
    let text = if media == "application/pdf" || bytes.starts_with(b"%PDF-") {
        // A damaged PDF may make the parser panic: it is then left unindexed.
        std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes).ok())
            .ok()
            .flatten()?
    } else if [".txt", ".md", ".markdown", ".csv", ".tsv", ".json", ".log"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        return None;
    };
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(text.chars().take(FILE_TEXT).collect())
}

/// One file not yet indexed: `(id, name, media type)`.
fn next_file(db: &Connection) -> Option<(String, String, String)> {
    db.query_row(
        "SELECT id, filename, media_type FROM attachments
        WHERE id NOT IN (SELECT attachment_id FROM file_text) LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
    .ok()
    .flatten()
}
/// Indexes one waiting file: reads it under the lock, extracts its words without
/// it, and stores them under it again. Returns whether there was one.
pub(super) fn index_next_file(store: &Shared) -> bool {
    let (id, name, media, bytes) = {
        let Ok(s) = store.lock() else { return false };
        let Some((id, name, media)) = next_file(&s.db) else {
            return false;
        };
        let bytes: Vec<u8> =
            s.db.query_row("SELECT content FROM attachments WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap_or_default();
        (id, name, media, bytes)
    };
    let text = file_words(&name, &media, &bytes);
    let Ok(s) = store.lock() else { return false };
    let still_there =
        s.db.query_row("SELECT 1 FROM attachments WHERE id=?1", [&id], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some();
    if still_there {
        let _ = s.db.execute(
            "INSERT OR REPLACE INTO file_text(attachment_id, indexed) VALUES(?1, ?2)",
            params![id, now()],
        );
        let _ =
            s.db.execute("DELETE FROM file_search WHERE attachment_id=?1", [&id]);
        let _ = s.db.execute(
            "INSERT INTO file_search(attachment_id, name, body) VALUES(?1, ?2, ?3)",
            params![id, name, text.unwrap_or_default()],
        );
    }
    true
}
/// Forgets a deleted file's words.
pub(super) fn forget_file(db: &Connection, id: &str) -> rusqlite::Result<()> {
    db.execute("DELETE FROM file_search WHERE attachment_id=?1", [id])?;
    db.execute("DELETE FROM file_text WHERE attachment_id=?1", [id])?;
    Ok(())
}
/// Forgets the words of every file of a deleted note.
pub(super) fn forget_files_of(db: &Connection, note_id: &str) -> rusqlite::Result<()> {
    for table in ["file_search", "file_text"] {
        db.execute(
            &format!(
                "DELETE FROM {table} WHERE attachment_id IN (SELECT id FROM attachments WHERE note_id=?1)"
            ),
            [note_id],
        )?;
    }
    Ok(())
}
/// Starts the worker that indexes files' text: it checks every 20 seconds, and
/// goes on at once while files wait.
pub(crate) fn spawn(store: Shared) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        loop {
            let worker = store.clone();
            let more = tokio::task::spawn_blocking(move || index_next_file(&worker))
                .await
                .unwrap_or(false);
            if !more {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(20)) => {}
                    _ = signal().notified() => {}
                }
            }
        }
    });
}
fn signal() -> &'static tokio::sync::Notify {
    static SIGNAL: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    SIGNAL.get_or_init(tokio::sync::Notify::new)
}
/// Tells the worker a file was added.
pub(super) fn wake() {
    signal().notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A one-page PDF showing `text` in Helvetica, with a correct cross-reference table.
    fn pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 18 Tf 72 720 Td ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![];
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }
    #[test]
    fn reads_pdfs_and_text_files_only() {
        let text = file_words(
            "scan.pdf",
            "application/pdf",
            &pdf("Quarterly invoice Minerva"),
        )
        .unwrap();
        assert!(text.contains("Quarterly invoice Minerva"), "{text}");
        assert_eq!(
            file_words(
                "notes.md",
                "application/octet-stream",
                b"# Title\n\n  some   words"
            )
            .as_deref(),
            Some("# Title some words")
        );
        assert_eq!(file_words("photo.png", "image/png", b"\x89PNG"), None);
        // A damaged PDF is skipped, never a crash.
        assert_eq!(
            file_words("bad.pdf", "application/pdf", b"%PDF-1.4 nonsense").unwrap_or_default(),
            ""
        );
    }
}
