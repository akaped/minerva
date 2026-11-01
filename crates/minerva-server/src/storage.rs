//! Persistence. `Data` lives in memory, and SQLite keeps it in tables so that a
//! change rewrites only what changed. Notes, their live editing state, databases,
//! replication clocks and sessions have a row each, keyed by ID; the rest of `Data`
//! (accounts, workspaces, folders, sharing…, all small) is one JSON row in
//! `settings`. Note text is also indexed for search (`search.rs`).
//!
//! Earlier versions of notes stay in SQLite only, a row each in `note_versions`,
//! up to `KEPT_VERSIONS` per note: they are read when someone opens a note's
//! history. `Data::history` holds just the versions a request adds until they are
//! saved.
//!
//! `PRAGMA user_version` names the layout, and older ones are migrated on open:
//! 0 is the original single `state` row (kept as `state_v0`); 1 kept each note's
//! history as one JSON row (`note_history`); 2 is this layout.
use super::*;
use std::mem::take;

const VERSION: i64 = 2;
/// Earlier versions kept per note.
pub(super) const KEPT_VERSIONS: usize = 50;
/// The `Data` fields with a row per entry: field name, table, and whether the field
/// is a list (kept in insertion order) rather than a map.
const TABLES: [(&str, &str, bool); 5] = [
    ("notes", "notes", true),
    ("collaboration", "note_crdt", false),
    ("databases", "databases", true),
    ("clocks", "field_clocks", false),
    ("sessions", "sessions", false),
];

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, payload TEXT NOT NULL);",
    )?;
    for (_, table, _) in TABLES {
        db.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {table}(id TEXT PRIMARY KEY, payload TEXT NOT NULL);"
        ))?;
    }
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS note_versions(
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            payload TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS note_versions_note ON note_versions(note_id, seq);",
    )?;
    search::initialize(db)
}

/// What `load` found: the data, its layout, and for an older layout, each note's
/// earlier versions (oldest first) to move into `note_versions`.
struct Loaded {
    data: Data,
    layout: i64,
    versions: Vec<(String, Vec<Value>)>,
}

/// Reads the stored data (or creates it with `initial`), lets `prepare` adjust it,
/// and saves the result, migrating older layouts. Returns the data and its saved
/// `settings` row.
pub(super) fn open(
    db: &Connection,
    initial: impl FnOnce() -> Result<Data, ApiError>,
    prepare: impl FnOnce(&mut Data) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(Data, String), Box<dyn std::error::Error>> {
    let (mut before, mut data, layout, versions) = match load(db)? {
        Some(l) if l.layout == VERSION => (l.data.clone(), l.data, VERSION, vec![]),
        Some(l) => (Data::default(), l.data, l.layout, l.versions),
        None => (
            Data::default(),
            initial().map_err(|e| e.1)?,
            VERSION,
            vec![],
        ),
    };
    // A migrated or new database has no rows yet: write everything.
    let saved = if before.server_id.is_empty() {
        String::new()
    } else {
        rest(&mut before).map_err(|e| e.1)?
    };
    prepare(&mut data)?;
    let tx = db.unchecked_transaction()?;
    let (_, saved) = write(&tx, &before, &mut data, &saved).map_err(|e| e.1)?;
    data.history.clear();
    for (id, list) in &versions {
        for version in list {
            insert_version(
                &tx,
                id,
                version["updated_at"].as_u64().unwrap_or(0),
                &version.to_string(),
            )
            .map_err(|e| e.1)?;
        }
        trim_versions(&tx, id).map_err(|e| e.1)?;
    }
    search::ensure(&tx, &data)?;
    match layout {
        0 => tx.execute_batch("ALTER TABLE state RENAME TO state_v0;")?,
        1 => tx.execute_batch("DROP TABLE note_history;")?,
        _ => {}
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
    let mut stmt =
        db.prepare("SELECT note_id, max(updated_at) FROM note_versions GROUP BY note_id")?;
    data.versioned_at = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok((data, saved))
}

/// The stored data; `None` for a new database.
fn load(db: &Connection) -> Result<Option<Loaded>, Box<dyn std::error::Error>> {
    let layout: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if layout > VERSION {
        return Err("This database was written by a newer version of Minerva".into());
    }
    if layout == 0 {
        let legacy: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='state')",
            [],
            |r| r.get(0),
        )?;
        let raw: Option<String> = if legacy {
            db.query_row("SELECT payload FROM state WHERE id=1", [], |r| r.get(0))
                .optional()?
        } else {
            None
        };
        let Some(raw) = raw else {
            return Ok(None);
        };
        let mut v: Value = serde_json::from_str(&raw)?;
        let versions = match v.as_object_mut().and_then(|o| o.remove("history")) {
            Some(Value::Object(map)) => map.into_iter().map(versions_of).collect(),
            _ => vec![],
        };
        return Ok(Some(Loaded {
            data: serde_json::from_value(v)?,
            layout,
            versions,
        }));
    }
    let raw: Option<String> = db
        .query_row("SELECT payload FROM settings WHERE key='data'", [], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut v: Value = serde_json::from_str(&raw)?;
    for (field, table, list) in TABLES {
        let mut items = vec![];
        let mut map = serde_json::Map::new();
        for (id, payload) in rows(db, table)? {
            if list {
                items.push(payload);
            } else {
                map.insert(id, payload);
            }
        }
        v[field] = if list {
            Value::Array(items)
        } else {
            Value::Object(map)
        };
    }
    let versions = if layout == 1 {
        rows(db, "note_history")?
            .into_iter()
            .map(versions_of)
            .collect()
    } else {
        vec![]
    };
    Ok(Some(Loaded {
        data: serde_json::from_value(v)?,
        layout,
        versions,
    }))
}
fn rows(db: &Connection, table: &str) -> Result<Vec<(String, Value)>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(&format!("SELECT id, payload FROM {table} ORDER BY rowid"))?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = vec![];
    for row in rows {
        let (id, payload) = row?;
        out.push((id, serde_json::from_str(&payload)?));
    }
    Ok(out)
}
fn versions_of((id, list): (String, Value)) -> (String, Vec<Value>) {
    match list {
        Value::Array(list) => (id, list),
        _ => (id, vec![]),
    }
}

fn insert_version(
    db: &Connection,
    id: &str,
    updated_at: u64,
    payload: &str,
) -> Result<(), ApiError> {
    db.prepare_cached("INSERT INTO note_versions(note_id, updated_at, payload) VALUES(?1, ?2, ?3)")
        .and_then(|mut s| s.execute(params![id, updated_at as i64, payload]))
        .map_err(|_| internal())?;
    Ok(())
}
fn trim_versions(db: &Connection, id: &str) -> Result<(), ApiError> {
    db.prepare_cached(
        "DELETE FROM note_versions WHERE note_id=?1 AND seq NOT IN
            (SELECT seq FROM note_versions WHERE note_id=?1 ORDER BY seq DESC LIMIT ?2)",
    )
    .and_then(|mut s| s.execute(params![id, KEPT_VERSIONS as i64]))
    .map_err(|_| internal())?;
    Ok(())
}
/// A note's saved earlier versions, oldest first.
pub(super) fn versions(db: &Connection, id: &str) -> Result<Vec<Value>, ApiError> {
    let mut stmt = db
        .prepare_cached("SELECT payload FROM note_versions WHERE note_id=?1 ORDER BY seq")
        .map_err(|_| internal())?;
    let rows = stmt
        .query_map([id], |r| r.get::<_, String>(0))
        .map_err(|_| internal())?;
    rows.map(|r| {
        r.map_err(|_| internal())
            .and_then(|p| serde_json::from_str(&p).map_err(|_| internal()))
    })
    .collect()
}
/// Calls `each` with the stored JSON of every saved earlier version.
pub(super) fn each_version(db: &Connection, mut each: impl FnMut(&str)) -> Result<(), ApiError> {
    let mut stmt = db
        .prepare("SELECT payload FROM note_versions")
        .map_err(|_| internal())?;
    let mut rows = stmt.query([]).map_err(|_| internal())?;
    while let Some(row) = rows.next().map_err(|_| internal())? {
        each(
            row.get_ref(0)
                .map_err(|_| internal())?
                .as_str()
                .map_err(|_| internal())?,
        );
    }
    Ok(())
}

/// `d` without the fields stored in their own tables, as JSON.
fn rest(d: &mut Data) -> Result<String, ApiError> {
    let parts = (
        take(&mut d.notes),
        take(&mut d.collaboration),
        take(&mut d.databases),
        take(&mut d.clocks),
        take(&mut d.sessions),
    );
    let rest = serde_json::to_string(d).map_err(|_| internal());
    (d.notes, d.collaboration, d.databases, d.clocks, d.sessions) = parts;
    rest
}

/// The entries of one table to write or delete.
struct Diff<'a, T> {
    put: Vec<(&'a str, &'a T)>,
    gone: Vec<&'a str>,
}
/// The entries of `new` that are not the `same` as in `old`, and those gone from it.
fn diff<'a, T: 'a>(
    old: impl Iterator<Item = (&'a str, &'a T)>,
    new: impl Iterator<Item = (&'a str, &'a T)>,
    same: impl Fn(&T, &T) -> bool,
) -> Diff<'a, T> {
    let mut before: HashMap<&str, &T> = old.collect();
    let put = new
        .filter(|(id, item)| !before.remove(id).is_some_and(|b| same(b, item)))
        .collect();
    Diff {
        put,
        gone: before.into_keys().collect(),
    }
}
/// Writes a diff to its table, keeping the row IDs of existing entries (the search
/// index refers to them); returns whether anything changed.
fn apply<T: Serialize>(db: &Connection, table: &str, diff: &Diff<T>) -> Result<bool, ApiError> {
    let mut upsert = db
        .prepare_cached(&format!(
            "INSERT INTO {table}(id, payload) VALUES(?1, ?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload"
        ))
        .map_err(|_| internal())?;
    for (id, item) in &diff.put {
        let payload = serde_json::to_string(item).map_err(|_| internal())?;
        upsert
            .execute(params![id, payload])
            .map_err(|_| internal())?;
    }
    let mut delete = db
        .prepare_cached(&format!("DELETE FROM {table} WHERE id=?1"))
        .map_err(|_| internal())?;
    for id in &diff.gone {
        delete.execute([id]).map_err(|_| internal())?;
    }
    Ok(!diff.put.is_empty() || !diff.gone.is_empty())
}
/// Shared values are unchanged while both copies of `Data` still share them.
pub(super) fn shared<T: PartialEq>(a: &Arc<T>, b: &Arc<T>) -> bool {
    Arc::ptr_eq(a, b) || a == b
}
fn keyed<T>(map: &HashMap<String, T>) -> impl Iterator<Item = (&str, &T)> {
    map.iter().map(|(k, v)| (k.as_str(), v))
}

/// Writes what differs between `old` (whose `settings` row is `saved`) and `new`.
/// Returns whether anything changed, and `new`'s `settings` row.
pub(super) fn write(
    db: &Connection,
    old: &Data,
    new: &mut Data,
    saved: &str,
) -> Result<(bool, String), ApiError> {
    let rest = rest(new)?;
    let new = &*new;
    let mut changed = rest != saved;
    if changed {
        db.execute(
            "INSERT OR REPLACE INTO settings(key, payload) VALUES('data', ?1)",
            [&rest],
        )
        .map_err(|_| internal())?;
    }
    let notes = diff(
        old.notes.iter().map(|n| (n.id.as_str(), n)),
        new.notes.iter().map(|n| (n.id.as_str(), n)),
        shared,
    );
    // Index rows are found through the note's row, so they go first and come back
    // after the note is written.
    for id in notes.gone.iter().chain(notes.put.iter().map(|(id, _)| id)) {
        search::remove(db, id).map_err(|_| internal())?;
    }
    changed |= apply(db, "notes", &notes)?;
    for (_, n) in &notes.put {
        search::index(db, n).map_err(|_| internal())?;
    }
    // Versions added by this change; a deleted note's versions go with it.
    for (id, list) in &new.history {
        for version in list {
            let payload = serde_json::to_string(version).map_err(|_| internal())?;
            insert_version(db, id, version.updated_at, &payload)?;
            changed = true;
        }
        trim_versions(db, id)?;
    }
    for id in &notes.gone {
        db.prepare_cached("DELETE FROM note_versions WHERE note_id=?1")
            .and_then(|mut s| s.execute([id]))
            .map_err(|_| internal())?;
    }
    let databases = diff(
        old.databases.iter().map(|b| (b.id.as_str(), b)),
        new.databases.iter().map(|b| (b.id.as_str(), b)),
        PartialEq::eq,
    );
    changed |= apply(db, "databases", &databases)?;
    changed |= apply(
        db,
        "note_crdt",
        &diff(keyed(&old.collaboration), keyed(&new.collaboration), shared),
    )?;
    changed |= apply(
        db,
        "field_clocks",
        &diff(keyed(&old.clocks), keyed(&new.clocks), PartialEq::eq),
    )?;
    changed |= apply(
        db,
        "sessions",
        &diff(keyed(&old.sessions), keyed(&new.sessions), PartialEq::eq),
    )?;
    Ok((changed, rest))
}
