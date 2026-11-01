//! Full-text search over note titles, text and tags with SQLite FTS5. Index rows
//! share their row ID with the note's row in `notes` (see `storage.rs`, which
//! keeps it stable and updates the index in the same transaction). Matching
//! ignores case and accents, and ranks title matches above tags above text.
use super::*;

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS note_search
            USING fts5(title, body, tags, tokenize='unicode61 remove_diacritics 2');",
    )
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
const INDEX_VERSION: &str = "2";
/// Rebuilds the index unless it holds one row per note, of this version.
pub(super) fn ensure(db: &Connection, d: &Data) -> rusqlite::Result<()> {
    let count: usize = db.query_row("SELECT count(*) FROM note_search", [], |r| r.get(0))?;
    let version: Option<String> = db
        .query_row("SELECT payload FROM settings WHERE key='search'", [], |r| {
            r.get(0)
        })
        .optional()?;
    if count != d.notes.len() || version.as_deref() != Some(INDEX_VERSION) {
        db.execute(
            "INSERT OR REPLACE INTO settings(key, payload) VALUES('search', ?1)",
            [INDEX_VERSION],
        )?;
        db.execute("DELETE FROM note_search", [])?;
        for n in &d.notes {
            index(db, n)?;
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
    // Quoted words are never read as FTS5 operators (AND, NEAR, column filters…).
    let expression = format!(
        "{}*",
        words
            .iter()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" ")
    );
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
