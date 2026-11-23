//! Comments on notes: threads on a block (quoting the text they are about, if
//! any) or on the whole note, with replies. Anyone who can read a note can
//! comment, and so can guests with an edit link (as "Guest", seeing members only
//! as "Member"); editors and a thread's author resolve it; authors edit and
//! delete their own comments. `@person@example.com` in a member's comment, and
//! earlier participants of the thread, get an email. Threads live in their own
//! table, not in `Data`, and go with their note; open pages hear about changes
//! through the change feed.
//!
//! Threads replicate to desktop offline copies as entities of kind `thread`
//! (see `replication.rs`): the thread's own fields, and a field per comment,
//! `comments/{id}`, which is null once deleted. Each merges last-writer-wins by
//! clock, so replies written in two places both survive.
use super::*;
use replication::{Clocks, Fields};

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
    /// When its author last changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edited_at: Option<u64>,
    /// The author's name when they wrote it, for copies that do not know them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    author_name: Option<String>,
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
/// Who is commenting: a member with their role on the note, or a share-link
/// guest (`user.id` is `share:{share id}`).
pub(super) struct Commenter {
    pub user: User,
    pub role: Role,
    pub guest: bool,
}
/// A thread for the page, with each author's name. Guests see members only as
/// "Member", and no one's ID but their own.
fn view(d: &Data, t: &Thread, by: &Commenter) -> Value {
    let name = |id: &str| {
        if by.guest && !id.starts_with("share:") {
            return "Member".to_string();
        }
        if id == by.user.id && !by.guest {
            return by.user.name.clone();
        }
        author_name(d, t, id)
    };
    let hide = |id: &str| -> String {
        if by.guest && id != by.user.id {
            "someone".into()
        } else {
            id.into()
        }
    };
    let mut v = json!(t);
    v["resolved_by_name"] = json!(t.resolved_by.as_deref().map(name));
    if by.guest {
        v["resolved_by"] = json!(t.resolved_by.as_deref().map(hide));
    }
    if let Some(list) = v["comments"].as_array_mut() {
        for (c, item) in t.comments.iter().zip(list) {
            item["author_name"] = json!(name(&c.author));
            item["author"] = json!(hide(&c.author));
            item["mine"] = json!(c.author == by.user.id);
        }
    }
    v
}
/// A participant's name: their account's, or the name saved with their
/// comments (an offline copy does not know the server's accounts).
fn author_name(d: &Data, t: &Thread, id: &str) -> String {
    if id.starts_with("share:") {
        return "Guest".into();
    }
    d.users
        .iter()
        .find(|a| a.user.id == id)
        .map(|a| a.user.name.clone())
        .or_else(|| {
            t.comments
                .iter()
                .find(|c| c.author == id)
                .and_then(|c| c.author_name.clone())
        })
        .unwrap_or_else(|| "Former member".into())
}
/// Tells open pages that the note's comments changed (see `live.rs`).
fn touched(d: &mut Data, note: &str) {
    *d.comment_activity.entry(note.into()).or_default() += 1;
}
/// Emails the people a comment mentions and the thread's earlier participants
/// who can still open the note; never its author. A guest's mentions email no
/// one: only the thread's participants hear from guests.
fn notify(d: &mut Data, by: &Commenter, note: &Note, t: &Thread, comment: &str) {
    let mut to: Vec<String> = if by.guest {
        vec![]
    } else {
        mail::mentions(comment)
    };
    let by = &by.user;
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
    let mut user = u.clone();
    // In an offline copy the owner comments as themselves on the server.
    if let Some(r) = replication::folder_workspace(d, &note.folder_id)
        .and_then(|w| replication::replica(d, &w))
        .filter(|r| !r.user_id.is_empty())
    {
        user.id = r.user_id.clone();
        if !r.user_name.is_empty() {
            user.name = r.user_name.clone();
        }
    }
    let by = Commenter {
        user,
        role,
        guest: false,
    };
    handle(db, d, &by, &note, method, rest, v).map(Some)
}
/// `shared/comments…` for a guest whose link edits the note.
#[allow(clippy::too_many_arguments)]
pub(super) fn guest_route(
    db: &Connection,
    d: &mut Data,
    share_id: &str,
    role: &Role,
    note: &Note,
    method: &str,
    rest: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    if *role != Role::Editor {
        return Err(forbidden());
    }
    let by = Commenter {
        user: User {
            token_workspace: None,
            id: format!("share:{share_id}"),
            name: "Guest".into(),
            email: String::new(),
            admin: false,
            can_create_workspace: false,
            disabled: false,
        },
        role: Role::Editor,
        guest: true,
    };
    handle(db, d, &by, note, method, rest, v)
}
fn handle(
    db: &Connection,
    d: &mut Data,
    by: &Commenter,
    note: &Note,
    method: &str,
    rest: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    let (u, role) = (&by.user, by.role.clone());
    let nid = &note.id.as_str();
    let mut all = threads(db, nid)?;
    let find = |all: &mut Vec<Thread>, id: &str| -> Result<usize, ApiError> {
        all.iter().position(|t| t.id == id).ok_or_else(missing)
    };
    let result = match (method, rest) {
        ("GET", []) => json!(all.iter().map(|t| view(d, t, by)).collect::<Vec<_>>()),
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
                    edited_at: None,
                    author_name: Some(u.name.clone()),
                }],
            };
            save(db, &t)?;
            record(db, d, None, Some(&t))?;
            notify(
                d,
                by,
                note,
                &Thread {
                    comments: vec![],
                    ..t.clone()
                },
                &comment,
            );
            touched(d, nid);
            view(d, &t, by)
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
                edited_at: None,
                author_name: Some(u.name.clone()),
            });
            // Replying to a resolved thread opens it again.
            t.resolved = false;
            t.resolved_by = None;
            save(db, t)?;
            let t = t.clone();
            record(db, d, Some(&before), Some(&t))?;
            notify(d, by, note, &before, &comment);
            touched(d, nid);
            view(d, &t, by)
        }
        // Authors change their own words; only people newly mentioned hear of it.
        ("PATCH", [tid, cid]) => {
            let i = find(&mut all, tid)?;
            let c = all[i]
                .comments
                .iter()
                .position(|c| c.id == *cid)
                .ok_or_else(missing)?;
            if all[i].comments[c].author != u.id {
                return Err(forbidden());
            }
            let comment = text(v)?;
            let previous = all[i].clone();
            let before = mail::mentions(&all[i].comments[c].text);
            all[i].comments[c].text = comment.clone();
            all[i].comments[c].edited_at = Some(now());
            save(db, &all[i])?;
            record(db, d, Some(&previous), Some(&all[i]))?;
            let added: Vec<String> = mail::mentions(&comment)
                .into_iter()
                .filter(|m| !before.contains(m))
                .collect();
            if !added.is_empty() {
                let only_new = Thread {
                    comments: vec![],
                    ..all[i].clone()
                };
                notify(
                    d,
                    by,
                    note,
                    &only_new,
                    &added
                        .iter()
                        .map(|m| format!("@{m}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            touched(d, nid);
            view(d, &all[i], by)
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
            let previous = all[i].clone();
            let t = &mut all[i];
            t.resolved = resolved;
            t.resolved_by = resolved.then(|| u.id.clone());
            save(db, t)?;
            let t = t.clone();
            record(db, d, Some(&previous), Some(&t))?;
            touched(d, nid);
            view(d, &t, by)
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
            let previous = all[i].clone();
            if c == 0 {
                db.execute("DELETE FROM comments WHERE id=?1", [tid])
                    .map_err(|_| internal())?;
                record(db, d, Some(&previous), None)?;
                touched(d, nid);
                return Ok(json!({"deleted": "thread"}));
            }
            all[i].comments.remove(c);
            save(db, &all[i])?;
            record(db, d, Some(&previous), Some(&all[i]))?;
            touched(d, nid);
            view(d, &all[i], by)
        }
        _ => return Err(missing()),
    };
    Ok(result)
}

/// Comments mentioning `u` on notes they can open, latest first: for the
/// Mentions widget, beside mentions in notes' text.
pub(super) fn mentioning(db: &Connection, d: &Data, u: &User) -> Vec<(u64, Value)> {
    let email = u.email.to_lowercase();
    if email.is_empty() {
        return vec![];
    }
    let Ok(mut stmt) =
        db.prepare("SELECT payload FROM comments WHERE lower(payload) LIKE ?1 LIMIT 500")
    else {
        return vec![];
    };
    let pattern = format!("%{}%", email.replace(['%', '_'], ""));
    let Ok(rows) = stmt.query_map([pattern], |r| r.get::<_, String>(0)) else {
        return vec![];
    };
    let mut out = vec![];
    for t in rows
        .flatten()
        .filter_map(|p| serde_json::from_str::<Thread>(&p).ok())
    {
        let Some(n) = d
            .notes
            .iter()
            .find(|n| n.id == t.note_id && !n.trashed && note_role(d, u, n).is_some())
        else {
            continue;
        };
        for c in t
            .comments
            .iter()
            .filter(|c| c.author != u.id && mail::mentions(&c.text).contains(&email))
        {
            let excerpt: String = c.text.chars().take(160).collect();
            out.push((
                c.created_at,
                json!({"id": n.id, "title": n.title, "icon": n.icon, "updated_at": c.created_at, "excerpt": excerpt, "comment": true, "thread": t.id, "block": t.block_id}),
            ));
        }
    }
    out
}

// Replication of threads to and from desktop offline copies.

fn load(db: &Connection, id: &str) -> Result<Option<Thread>, ApiError> {
    db.query_row("SELECT payload FROM comments WHERE id=?1", [id], |r| {
        r.get::<_, String>(0)
    })
    .optional()
    .map_err(|_| internal())?
    .map(|p| serde_json::from_str(&p).map_err(|_| internal()))
    .transpose()
}
fn workspace_of(d: &Data, note_id: &str) -> Option<String> {
    let note = d.notes.iter().find(|n| n.id == note_id)?;
    replication::folder_workspace(d, &note.folder_id)
}
const REPLY: &str = "comments/";
/// A thread as independently merging fields. Deleted comments, which still have
/// a clock, are null.
fn flatten(d: &Data, t: &Thread, clocks: Option<&Clocks>) -> Fields {
    let mut f = Fields::new();
    f.insert("note_id".into(), json!(t.note_id));
    f.insert("block_id".into(), json!(t.block_id));
    f.insert("quote".into(), json!(t.quote));
    f.insert("created_at".into(), json!(t.created_at));
    f.insert("resolved".into(), json!(t.resolved));
    f.insert("resolved_by".into(), json!(t.resolved_by));
    for name in clocks.into_iter().flatten().map(|(k, _)| k) {
        if name.starts_with(REPLY) {
            f.insert(name.clone(), Value::Null);
        }
    }
    for c in &t.comments {
        f.insert(
            format!("{REPLY}{}", c.id),
            json!({
                "author": c.author,
                "author_name": author_name(d, t, &c.author),
                "text": c.text,
                "created_at": c.created_at,
                "edited_at": c.edited_at,
            }),
        );
    }
    f
}
/// The thread's field clocks. Threads from before replication have none, and
/// get clocks from their timestamps.
fn clocks_of(d: &Data, t: &Thread) -> Clocks {
    let mut clocks = d
        .clocks
        .get(&replication::key("thread", &t.id))
        .map(|c| (**c).clone())
        .unwrap_or_default();
    let at = |secs: u64| format!("{:013}.000000.{}", secs * 1000, d.server_id);
    for (name, _) in flatten(d, t, Some(&clocks)) {
        if clocks.contains_key(&name) {
            continue;
        }
        let secs = name
            .strip_prefix(REPLY)
            .and_then(|cid| t.comments.iter().find(|c| c.id == cid))
            .map_or(t.created_at, |c| c.edited_at.unwrap_or(c.created_at));
        clocks.insert(name, at(secs));
    }
    clocks
}
fn set_clocks(d: &mut Data, id: &str, clocks: impl IntoIterator<Item = (String, String)>) {
    let entry = Arc::make_mut(&mut d.clocks)
        .entry(replication::key("thread", id))
        .or_default();
    Arc::make_mut(entry).extend(clocks);
}
/// Stamps the fields a request changed and logs the thread for replicas.
fn record(
    db: &Connection,
    d: &mut Data,
    before: Option<&Thread>,
    after: Option<&Thread>,
) -> Result<(), ApiError> {
    let Some(t) = after.or(before) else {
        return Ok(());
    };
    let Some(ws) = workspace_of(d, &t.note_id) else {
        return Ok(());
    };
    let id = t.id.clone();
    let Some(after) = after else {
        let clock = replication::tick(d);
        Arc::make_mut(&mut d.clocks).remove(&replication::key("thread", &id));
        return replication::log_entity(db, &ws, "thread", &id, Some(clock), false);
    };
    let old = before.map(|b| flatten(d, b, None)).unwrap_or_default();
    let new = flatten(d, after, None);
    let changed: Vec<String> = new
        .iter()
        .filter(|(k, v)| old.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .chain(old.keys().filter(|k| !new.contains_key(*k)).cloned())
        .collect();
    let stamped: Vec<(String, String)> = changed
        .into_iter()
        .map(|f| (f, replication::tick(d)))
        .collect();
    set_clocks(d, &id, stamped);
    replication::log_entity(db, &ws, "thread", &id, None, false)
}
/// Logs threads from before replication, so a replica's first pull gets them.
pub(super) fn backfill(db: &Connection, d: &Data) -> rusqlite::Result<()> {
    let missing: Vec<(String, String)> = db
        .prepare(
            "SELECT id, note_id FROM comments c WHERE NOT EXISTS(
                SELECT 1 FROM changes WHERE kind='thread' AND entity_id=c.id)",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, note) in missing {
        if let Some(ws) = workspace_of(d, &note) {
            replication::log_entity(db, &ws, "thread", &id, None, false)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
        }
    }
    Ok(())
}
/// The pulled entry for a thread, or a tombstone when it is gone or `u` cannot
/// read its note.
pub(super) fn sync_entry(db: &Connection, d: &Data, u: &User, id: &str, clock: &str) -> Value {
    let tombstone = json!({"kind":"thread","id":id,"deleted":true,"clocks":{"deleted":clock}});
    let Ok(Some(t)) = load(db, id) else {
        return tombstone;
    };
    let Some(note) = d.notes.iter().find(|n| n.id == t.note_id) else {
        return tombstone;
    };
    let Some(role) = note_role(d, u, note) else {
        return tombstone;
    };
    let clocks = clocks_of(d, &t);
    json!({
        "kind": "thread",
        "id": id,
        "fields": flatten(d, &t, Some(&clocks)),
        "clocks": clocks,
        "role": role,
    })
}
/// Applies a pushed thread: each field newer than ours. On a server, people
/// add, change and delete only their own comments, and resolve as they may on
/// the page; a replica takes its server's thread as it is (`upstream`).
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_synced(
    db: &Connection,
    d: &mut Data,
    u: &User,
    ws: &str,
    id: &str,
    fields: &Fields,
    clocks: &Clocks,
    upstream: bool,
) -> Result<Value, ApiError> {
    let existing = load(db, id)?;
    let note_id = match &existing {
        Some(t) => t.note_id.clone(),
        None => fields
            .get("note_id")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| bad("Missing note"))?,
    };
    let note = d
        .notes
        .iter()
        .find(|n| n.id == note_id)
        .map(|n| Note::clone(n))
        .ok_or_else(missing)?;
    if replication::folder_workspace(d, &note.folder_id).as_deref() != Some(ws) {
        return Err(bad("The entity belongs to another workspace"));
    }
    let role = if upstream {
        Role::Editor
    } else {
        note_role(d, u, &note).ok_or_else(forbidden)?
    };
    if existing.is_none() && !replication::canonical(id) {
        return Err(bad("New entities need a UUID"));
    }
    let ours = existing
        .as_ref()
        .map(|t| clocks_of(d, t))
        .unwrap_or_default();
    let current = existing
        .as_ref()
        .map(|t| flatten(d, t, Some(&ours)))
        .unwrap_or_default();
    let before = existing.clone();
    let string = |name: &str| fields.get(name).and_then(Value::as_str).map(String::from);
    let mut t = existing.unwrap_or_else(|| Thread {
        id: id.into(),
        note_id: note_id.clone(),
        block_id: string("block_id").filter(|b| !b.is_empty() && b.len() <= 80),
        quote: string("quote")
            .unwrap_or_default()
            .trim()
            .chars()
            .take(QUOTE)
            .collect(),
        resolved: false,
        resolved_by: None,
        created_at: fields
            .get("created_at")
            .and_then(Value::as_u64)
            .unwrap_or_else(now)
            .min(now()),
        comments: vec![],
    });
    let starter = t.comments.first().map(|c| c.author.clone());
    let started = t.comments.first().map(|c| c.created_at);
    let mut lost = vec![];
    let mut applied = Clocks::new();
    let mut added = vec![];
    for (name, value) in fields {
        let clock = &clocks[name];
        if clock.as_str() <= ours.get(name).map_or("", String::as_str) {
            // An older value; only a different one is a loss.
            let now = current.get(name).unwrap_or(&Value::Null);
            let differs = if name.starts_with(REPLY) {
                now["text"] != value["text"]
            } else {
                now != value
            };
            if differs {
                lost.push(name.clone());
            }
            continue;
        }
        match name.as_str() {
            "resolved" => {
                let starter = starter.as_deref().unwrap_or(u.id.as_str());
                if !upstream && role != Role::Editor && starter != u.id {
                    lost.push(name.clone());
                    // Refused: our value gets a new clock, so the replica takes it.
                    applied.insert(name.clone(), clock.clone());
                    applied.insert("resolved_by".into(), clock.clone());
                    continue;
                }
                t.resolved = value.as_bool().unwrap_or(false);
                if !upstream {
                    t.resolved_by = t.resolved.then(|| u.id.clone());
                    applied.insert("resolved_by".into(), clock.clone());
                }
            }
            // A server sets it from who resolved the thread.
            "resolved_by" if !upstream => continue,
            "resolved_by" => t.resolved_by = value.as_str().map(String::from),
            reply if reply.starts_with(REPLY) => {
                let cid = &reply[REPLY.len()..];
                if cid.is_empty() || cid.len() > 80 {
                    continue;
                }
                let at = t.comments.iter().position(|c| c.id == cid);
                let mine = |c: &Comment| upstream || c.author == u.id;
                match at {
                    None if value.is_null() => {}
                    Some(i) if value.is_null() => {
                        if !mine(&t.comments[i]) {
                            lost.push(name.clone());
                            applied.insert(name.clone(), clock.clone());
                            continue;
                        }
                        t.comments.remove(i);
                    }
                    None => {
                        let Ok(words) = text(value) else {
                            lost.push(name.clone());
                            applied.insert(name.clone(), clock.clone());
                            continue;
                        };
                        if !upstream && t.comments.len() >= REPLIES {
                            lost.push(name.clone());
                            applied.insert(name.clone(), clock.clone());
                            continue;
                        }
                        let (author, author_name) = if upstream {
                            (
                                value["author"].as_str().unwrap_or("").to_string(),
                                value["author_name"].as_str().map(String::from),
                            )
                        } else {
                            (u.id.clone(), Some(u.name.clone()))
                        };
                        t.comments.push(Comment {
                            id: cid.into(),
                            author,
                            text: words.clone(),
                            // Never before the thread's first comment, whose
                            // author started it.
                            created_at: value["created_at"]
                                .as_u64()
                                .unwrap_or_else(now)
                                .clamp(started.unwrap_or(0), now().max(started.unwrap_or(0))),
                            edited_at: value["edited_at"].as_u64().map(|e| e.min(now())),
                            author_name,
                        });
                        added.push(words);
                    }
                    Some(i) => {
                        if !mine(&t.comments[i]) {
                            lost.push(name.clone());
                            applied.insert(name.clone(), clock.clone());
                            continue;
                        }
                        let Ok(words) = text(value) else {
                            lost.push(name.clone());
                            applied.insert(name.clone(), clock.clone());
                            continue;
                        };
                        let c = &mut t.comments[i];
                        c.text = words;
                        c.edited_at =
                            Some(value["edited_at"].as_u64().unwrap_or_else(now).min(now()));
                        if upstream {
                            c.author_name = value["author_name"].as_str().map(String::from);
                        }
                    }
                }
            }
            // Where it is and what it quotes are set when it is started.
            _ => {}
        }
        applied.insert(name.clone(), clock.clone());
    }
    if applied.is_empty() {
        return Ok(replication::outcome(lost));
    }
    // Replies in time order; an existing thread keeps its first comment first.
    let pinned = usize::from(before.as_ref().is_some_and(|b| {
        !b.comments.is_empty()
            && t.comments.first().map(|c| &c.id) == b.comments.first().map(|c| &c.id)
    }));
    t.comments[pinned..].sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    // A thread whose comments are all deleted is gone.
    if t.comments.is_empty() {
        if before.is_none() {
            return Err(bad("A thread needs a comment"));
        }
        db.execute("DELETE FROM comments WHERE id=?1", [id])
            .map_err(|_| internal())?;
        let clock = replication::tick(d);
        Arc::make_mut(&mut d.clocks).remove(&replication::key("thread", id));
        replication::log_entity(db, ws, "thread", id, Some(clock), upstream)?;
        touched(d, &note_id);
        return Ok(replication::outcome(lost));
    }
    if before.is_none() && !upstream && threads(db, &note_id)?.len() >= THREADS {
        return Err(bad(
            "This note has 500 comment threads; resolve and delete some first",
        ));
    }
    save(db, &t)?;
    // A value stored differently from the pushed one (its author, say) gets a new
    // clock, so every copy converges on it.
    let stored = flatten(d, &t, None);
    let stamped: Vec<(String, String)> = applied
        .into_iter()
        .map(|(name, clock)| {
            let same = match (stored.get(&name), fields.get(&name)) {
                (Some(a), Some(b)) if name.starts_with(REPLY) => {
                    a["author"] == b["author"] && a["text"] == b["text"]
                }
                (a, b) => a.unwrap_or(&Value::Null) == b.unwrap_or(&Value::Null),
            };
            let clock = if same { clock } else { replication::tick(d) };
            (name, clock)
        })
        .collect();
    set_clocks(d, id, stamped);
    replication::log_entity(db, ws, "thread", id, None, upstream)?;
    touched(d, &note_id);
    if !upstream {
        let by = Commenter {
            user: u.clone(),
            role,
            guest: false,
        };
        let earlier = before.unwrap_or(Thread {
            comments: vec![],
            ..t.clone()
        });
        for words in added {
            notify(d, &by, &note, &earlier, &words);
        }
    }
    Ok(replication::outcome(lost))
}
/// Deletes a pushed thread. On a server only the person who started it may, and
/// a comment written or changed after the deletion keeps it.
#[allow(clippy::too_many_arguments)]
pub(super) fn delete_synced(
    db: &Connection,
    d: &mut Data,
    u: &User,
    ws: &str,
    id: &str,
    clock: &str,
    upstream: bool,
) -> Result<Value, ApiError> {
    let Some(t) = load(db, id)? else {
        return Ok(json!({"status":"applied"}));
    };
    if workspace_of(d, &t.note_id).as_deref() != Some(ws) {
        return Err(bad("The entity belongs to another workspace"));
    }
    if !upstream {
        let note = d
            .notes
            .iter()
            .find(|n| n.id == t.note_id)
            .ok_or_else(missing)?;
        note_role(d, u, note).ok_or_else(forbidden)?;
        if t.comments.first().map(|c| c.author.as_str()) != Some(u.id.as_str()) {
            return Err(forbidden());
        }
        let deleted_at = replication::parse(clock).0 / 1000;
        if t.comments
            .iter()
            .any(|c| c.edited_at.unwrap_or(c.created_at) > deleted_at)
        {
            return Ok(
                json!({"status":"conflict","kept":true,"entity":sync_entry(db, d, u, id, clock)}),
            );
        }
    }
    db.execute("DELETE FROM comments WHERE id=?1", [id])
        .map_err(|_| internal())?;
    Arc::make_mut(&mut d.clocks).remove(&replication::key("thread", id));
    replication::log_entity(db, ws, "thread", id, Some(clock.into()), upstream)?;
    touched(d, &t.note_id);
    Ok(json!({"status":"applied"}))
}
