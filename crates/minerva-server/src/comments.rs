//! Comments on notes: threads on a block (quoting the text they are about, if
//! any) or on the whole note, with replies. Anyone who can read a note can
//! comment; editors and a thread's author resolve it; authors delete their own
//! comments. `@person@example.com` in a comment, and earlier participants of the
//! thread, get an email. Threads live in their own table, not in `Data`, and go
//! with their note; open pages hear about changes through the change feed.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
struct Thread {
    id: String,
    note_id: String,
    /// The block the thread is about; none for the whole note.
    block_id: Option<String>,
    /// The text selected when the thread was started.
    quote: String,
    resolved: bool,
    resolved_by: Option<String>,
    created_at: u64,
    comments: Vec<Comment>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Comment {
    id: String,
    author: String,
    text: String,
    created_at: u64,
}

const TEXT: usize = 4000;
const QUOTE: usize = 500;
const THREADS: usize = 500;
const REPLIES: usize = 200;

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS comments(
            id TEXT PRIMARY KEY, note_id TEXT NOT NULL, payload TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS comments_note ON comments(note_id);",
    )
}
fn threads(db: &Connection, note: &str) -> Result<Vec<Thread>, ApiError> {
    let mut stmt = db
        .prepare_cached("SELECT payload FROM comments WHERE note_id=?1 ORDER BY rowid")
        .map_err(|_| internal())?;
    let rows = stmt
        .query_map([note], |r| r.get::<_, String>(0))
        .map_err(|_| internal())?;
    rows.map(|r| {
        r.map_err(|_| internal())
            .and_then(|p| serde_json::from_str(&p).map_err(|_| internal()))
    })
    .collect()
}
fn save(db: &Connection, t: &Thread) -> Result<(), ApiError> {
    db.execute(
        "INSERT INTO comments(id, note_id, payload) VALUES(?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",
        params![
            t.id,
            t.note_id,
            serde_json::to_string(t).map_err(|_| internal())?
        ],
    )
    .map_err(|_| internal())?;
    Ok(())
}
fn text(v: &Value) -> Result<String, ApiError> {
    let text = v["text"].as_str().unwrap_or("").trim();
    if text.is_empty() || text.chars().count() > TEXT {
        return Err(bad("A comment has 1–4,000 characters"));
    }
    Ok(text.to_string())
}
/// A thread for the page, with each author's name.
fn view(d: &Data, t: &Thread) -> Value {
    let name = |id: &str| {
        d.users
            .iter()
            .find(|a| a.user.id == id)
            .map_or("Former member".to_string(), |a| a.user.name.clone())
    };
    let mut v = json!(t);
    v["resolved_by_name"] = json!(t.resolved_by.as_deref().map(name));
    if let Some(list) = v["comments"].as_array_mut() {
        for (c, item) in t.comments.iter().zip(list) {
            item["author_name"] = json!(name(&c.author));
        }
    }
    v
}
/// Tells open pages that the note's comments changed (see `live.rs`).
fn touched(d: &mut Data, note: &str) {
    *d.comment_activity.entry(note.into()).or_default() += 1;
}
/// Emails the people a comment mentions and the thread's earlier participants
/// who can still open the note; never its author.
fn notify(d: &mut Data, by: &User, note: &Note, t: &Thread, comment: &str) {
    let mut to: Vec<String> = mail::mentions(comment);
    for c in &t.comments {
        if let Some(a) = d.users.iter().find(|a| a.user.id == c.author) {
            if !to.contains(&a.user.email) {
                to.push(a.user.email.clone());
            }
        }
    }
    let people: Vec<User> = to
        .iter()
        .filter_map(|email| {
            d.users
                .iter()
                .find(|a| &a.user.email == email && !a.user.disabled && a.user.id != by.id)
                .map(|a| a.user.clone())
        })
        .filter(|u| note_role(d, u, note).is_some())
        .take(20)
        .collect();
    for person in people {
        mail::comment(d, by, &person, note, comment);
    }
}

/// `notes/{id}/comments…`. Returns `None` for other paths.
pub(super) fn route(
    db: &Connection,
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Option<Value>, ApiError> {
    let ["notes", nid, "comments", rest @ ..] = parts else {
        return Ok(None);
    };
    let note = d
        .notes
        .iter()
        .find(|n| n.id == *nid && !n.trashed)
        .map(|n| Note::clone(n))
        .ok_or_else(missing)?;
    let role = note_role(d, u, &note).ok_or_else(forbidden)?;
    let mut all = threads(db, nid)?;
    let find = |all: &mut Vec<Thread>, id: &str| -> Result<usize, ApiError> {
        all.iter().position(|t| t.id == id).ok_or_else(missing)
    };
    let result = match (method, rest) {
        ("GET", []) => json!(all.iter().map(|t| view(d, t)).collect::<Vec<_>>()),
        // A new thread, on a block (and the text quoted from it) or the note.
        ("POST", []) => {
            if all.len() >= THREADS {
                return Err(bad(
                    "This note has 500 comment threads; resolve and delete some first",
                ));
            }
            let block_id = v["block_id"]
                .as_str()
                .filter(|b| !b.is_empty())
                .map(String::from);
            if block_id
                .as_ref()
                .is_some_and(|b| !note.blocks.iter().any(|x| &x.id == b))
            {
                return Err(bad("That block is no longer in the note"));
            }
            let quote: String = v["quote"]
                .as_str()
                .unwrap_or("")
                .trim()
                .chars()
                .take(QUOTE)
                .collect();
            let comment = text(v)?;
            let t = Thread {
                id: id(),
                note_id: note.id.clone(),
                block_id,
                quote,
                resolved: false,
                resolved_by: None,
                created_at: now(),
                comments: vec![Comment {
                    id: id(),
                    author: u.id.clone(),
                    text: comment.clone(),
                    created_at: now(),
                }],
            };
            save(db, &t)?;
            notify(
                d,
                u,
                &note,
                &Thread {
                    comments: vec![],
                    ..t.clone()
                },
                &comment,
            );
            touched(d, nid);
            view(d, &t)
        }
        ("POST", [tid, "replies"]) => {
            let i = find(&mut all, tid)?;
            if all[i].comments.len() >= REPLIES {
                return Err(bad("This thread has 200 replies; start a new one"));
            }
            let comment = text(v)?;
            let before = all[i].clone();
            let t = &mut all[i];
            t.comments.push(Comment {
                id: id(),
                author: u.id.clone(),
                text: comment.clone(),
                created_at: now(),
            });
            // Replying to a resolved thread opens it again.
            t.resolved = false;
            t.resolved_by = None;
            save(db, t)?;
            let t = t.clone();
            notify(d, u, &note, &before, &comment);
            touched(d, nid);
            view(d, &t)
        }
        ("PATCH", [tid]) => {
            let i = find(&mut all, tid)?;
            let started_by =
                all[i].comments.first().map(|c| c.author.as_str()) == Some(u.id.as_str());
            if role != Role::Editor && !started_by {
                return Err(forbidden());
            }
            let resolved = v["resolved"]
                .as_bool()
                .ok_or_else(|| bad("Say whether it is resolved"))?;
            let t = &mut all[i];
            t.resolved = resolved;
            t.resolved_by = resolved.then(|| u.id.clone());
            save(db, t)?;
            let t = t.clone();
            touched(d, nid);
            view(d, &t)
        }
        // Deleting the first comment deletes the thread.
        ("DELETE", [tid, cid]) => {
            let i = find(&mut all, tid)?;
            let c = all[i]
                .comments
                .iter()
                .position(|c| c.id == *cid)
                .ok_or_else(missing)?;
            if all[i].comments[c].author != u.id {
                return Err(forbidden());
            }
            if c == 0 {
                db.execute("DELETE FROM comments WHERE id=?1", [tid])
                    .map_err(|_| internal())?;
                touched(d, nid);
                return Ok(Some(json!({"deleted": "thread"})));
            }
            all[i].comments.remove(c);
            save(db, &all[i])?;
            touched(d, nid);
            view(d, &all[i])
        }
        _ => return Err(missing()),
    };
    Ok(Some(result))
}
