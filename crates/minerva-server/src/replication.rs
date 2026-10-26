//! Offline replication between desktop replicas and a server.
//!
//! Every persisted change replaces the row of each changed entity in the `changes`
//! log (deleted entities leave a tombstone row) and stamps each changed field with
//! a hybrid logical clock, `{wall_ms:013}.{counter:06}.{node}`, so clocks compare
//! as strings. Replicas pull entries changed since a cursor and push the entries
//! they changed; both directions use the same entry format:
//! `{kind, id, fields, clocks, crdt?, deleted?}`.
//!
//! Note title, text, block order, icon, favorite, trash, cover, and tags merge
//! through the note's Yrs document. Every other field merges last-writer-wins by
//! clock. When a pushed database cell that the replica changed loses to a different
//! value, the pushed values are kept as an archived conflict copy of the record.
//! Deletes lose to edits made after them: the entity is kept and returned so the
//! replica restores it.
use super::*;
use std::collections::BTreeMap;

type Fields = BTreeMap<String, Value>;
type Clocks = BTreeMap<String, String>;

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS changes(
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            deleted INTEGER NOT NULL,
            upstream INTEGER NOT NULL,
            clock TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS changes_workspace ON changes(workspace_id, seq);
        CREATE INDEX IF NOT EXISTS changes_entity ON changes(kind, entity_id);",
    )
}
pub(super) fn is_empty(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row("SELECT NOT EXISTS(SELECT 1 FROM changes)", [], |r| r.get(0))
}

/// One synced entity, flattened into the fields that merge independently.
struct Entity {
    kind: &'static str,
    id: String,
    workspace: String,
    fields: Fields,
}
fn key(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}
fn folder_workspace(d: &Data, folder: &str) -> Option<String> {
    d.folders
        .iter()
        .find(|f| f.id == folder)
        .map(|f| f.workspace_id.clone())
}
fn workspace_entity(d: &Data, w: &Workspace) -> Entity {
    Entity {
        kind: "workspace",
        id: w.id.clone(),
        workspace: w.id.clone(),
        fields: Fields::from([
            ("name".into(), json!(w.name)),
            ("journal_folder".into(), json!(d.journal_folders.get(&w.id))),
            (
                "template_folder".into(),
                json!(d.template_folders.get(&w.id)),
            ),
            (
                "journal_template".into(),
                json!(d.journal_templates.get(&w.id)),
            ),
        ]),
    }
}
/// The workspace's Journal and Templates folders and journal template, so that
/// replicas use the same ones instead of each creating their own.
const SPECIAL: [&str; 3] = ["journal_folder", "template_folder", "journal_template"];
fn set_special(d: &mut Data, ws: &str, fields: &Fields) {
    for name in SPECIAL {
        let Some(value) = fields.get(name) else {
            continue;
        };
        let map = match name {
            "journal_folder" => &mut d.journal_folders,
            "template_folder" => &mut d.template_folders,
            _ => &mut d.journal_templates,
        };
        match value.as_str().filter(|v| canonical(v)) {
            Some(id) => map.insert(ws.into(), id.into()),
            None => map.remove(ws),
        };
    }
    // Never point at another workspace's folder or note. An ID that does not exist
    // yet is kept: replicas may receive the folder after the workspace.
    let foreign = |d: &Data, id: &str| {
        let folder = |f: &str| folder_workspace(d, f).is_some_and(|w| w != ws);
        folder(id) || d.notes.iter().any(|n| n.id == id && folder(&n.folder_id))
    };
    for map in [0, 1, 2] {
        let id = match map {
            0 => d.journal_folders.get(ws),
            1 => d.template_folders.get(ws),
            _ => d.journal_templates.get(ws),
        };
        if id.is_some_and(|id| foreign(d, id)) {
            match map {
                0 => d.journal_folders.remove(ws),
                1 => d.template_folders.remove(ws),
                _ => d.journal_templates.remove(ws),
            };
        }
    }
}
/// Replicas choose the IDs of entities they create; they must be canonical
/// UUIDs, like the server's own, since pages place IDs in markup and URLs.
fn canonical(id: &str) -> bool {
    Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id)
}
fn folder_entity(f: &Folder) -> Entity {
    Entity {
        kind: "folder",
        id: f.id.clone(),
        workspace: f.workspace_id.clone(),
        fields: Fields::from([
            ("name".into(), json!(f.name)),
            ("parent_id".into(), json!(f.parent_id)),
        ]),
    }
}
fn note_entity(d: &Data, n: &Note) -> Option<Entity> {
    Some(Entity {
        kind: "note",
        id: n.id.clone(),
        workspace: folder_workspace(d, &n.folder_id)?,
        fields: Fields::from([
            ("folder_id".into(), json!(n.folder_id)),
            ("journal".into(), json!(n.journal)),
            ("metadata".into(), json!(n.metadata)),
            ("print_template".into(), json!(n.print_template)),
        ]),
    })
}
fn database_entity(d: &Data, db: &Database) -> Option<Entity> {
    Some(Entity {
        kind: "database",
        id: db.id.clone(),
        workspace: folder_workspace(d, &db.folder_id)?,
        fields: Fields::from([
            ("folder_id".into(), json!(db.folder_id)),
            ("name".into(), json!(db.name)),
            ("columns".into(), json!(db.columns)),
        ]),
    })
}
fn record_entity(workspace: &str, db: &Database, r: &Record) -> Entity {
    let mut fields = Fields::from([
        ("database_id".into(), json!(db.id)),
        ("archived".into(), json!(r.archived)),
        ("conflict_of".into(), json!(r.conflict_of)),
    ]);
    for (column, value) in &r.cells {
        fields.insert(format!("cells/{column}"), value.clone());
    }
    Entity {
        kind: "record",
        id: r.id.clone(),
        workspace: workspace.into(),
        fields,
    }
}
fn entities(d: &Data) -> Vec<Entity> {
    let mut out: Vec<Entity> = d
        .workspaces
        .iter()
        .map(|w| workspace_entity(d, w))
        .collect();
    out.extend(d.folders.iter().map(folder_entity));
    out.extend(d.notes.iter().filter_map(|n| note_entity(d, n)));
    for db in &d.databases {
        if let Some(e) = database_entity(d, db) {
            out.extend(db.rows.iter().map(|r| record_entity(&e.workspace, db, r)));
            out.push(e);
        }
    }
    out
}
fn find_record<'a>(d: &'a Data, id: &str) -> Option<(&'a Database, &'a Record)> {
    d.databases
        .iter()
        .find_map(|db| db.rows.iter().find(|r| r.id == id).map(|r| (db, r)))
}
fn current(d: &Data, kind: &str, id: &str) -> Option<Entity> {
    match kind {
        "workspace" => d
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| workspace_entity(d, w)),
        "folder" => d.folders.iter().find(|f| f.id == id).map(folder_entity),
        "note" => note_entity(d, d.notes.iter().find(|n| n.id == id)?),
        "database" => database_entity(d, d.databases.iter().find(|db| db.id == id)?),
        "record" => {
            let (db, r) = find_record(d, id)?;
            Some(record_entity(&folder_workspace(d, &db.folder_id)?, db, r))
        }
        _ => None,
    }
}

fn parse(clock: &str) -> (u64, u64) {
    let mut parts = clock.splitn(3, '.');
    let mut next = || parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (next(), next())
}
fn valid_clock(clock: &str) -> bool {
    let parts: Vec<_> = clock.splitn(3, '.').collect();
    clock.len() <= 100
        && parts.len() == 3
        && parts[0].len() == 13
        && parts[1].len() == 6
        && parts[..2]
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_digit()))
        && !parts[2].is_empty()
}
/// Issues a clock later than every clock this node has issued or seen.
fn tick(d: &mut Data) -> String {
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let (last, counter) = parse(&d.hlc);
    let (wall, counter) = if wall > last {
        (wall, 0)
    } else {
        (last, counter + 1)
    };
    d.hlc = format!("{wall:013}.{counter:06}.{}", d.server_id);
    d.hlc.clone()
}
fn observe(d: &mut Data, clock: &str) {
    let (wall, counter) = parse(clock);
    if (wall, counter) > parse(&d.hlc) {
        d.hlc = format!("{wall:013}.{counter:06}.{}", d.server_id);
    }
}

/// A change-log row.
pub(super) struct Change {
    workspace: String,
    kind: &'static str,
    id: String,
    deleted: bool,
    clock: String,
}
/// Compares two snapshots, stamps changed fields that were not given a clock
/// explicitly, and returns the change-log rows.
pub(super) fn track(old: &Data, new: &mut Data) -> Result<Vec<Change>, ApiError> {
    collaboration::ensure(new)?;
    let mut before: HashMap<String, Entity> = entities(old)
        .into_iter()
        .map(|e| (key(e.kind, &e.id), e))
        .collect();
    let mut changes = vec![];
    for e in entities(new) {
        let k = key(e.kind, &e.id);
        let previous = before.remove(&k).map(|p| p.fields).unwrap_or_default();
        let mut changed: Vec<String> = e
            .fields
            .iter()
            .filter(|(f, v)| previous.get(*f) != Some(*v))
            .map(|(f, _)| f.clone())
            .collect();
        changed.extend(
            previous
                .keys()
                .filter(|f| !e.fields.contains_key(*f))
                .cloned(),
        );
        let text = e.kind == "note" && old.collaboration.get(&e.id) != new.collaboration.get(&e.id);
        if changed.is_empty() && !text {
            continue;
        }
        for f in changed {
            let explicit = new.clocks.get(&k).and_then(|c| c.get(&f))
                != old.clocks.get(&k).and_then(|c| c.get(&f));
            if !explicit {
                let clock = tick(new);
                new.clocks.entry(k.clone()).or_default().insert(f, clock);
            }
        }
        changes.push(Change {
            workspace: e.workspace,
            kind: e.kind,
            id: e.id,
            deleted: false,
            clock: String::new(),
        });
    }
    for (k, e) in before {
        new.clocks.remove(&k);
        changes.push(Change {
            workspace: e.workspace,
            kind: e.kind,
            id: e.id,
            deleted: true,
            clock: tick(new),
        });
    }
    Ok(changes)
}
/// Replaces each entity's change-log row. A local change stays local when changes
/// pulled from upstream are applied on top of it, so it is still pushed.
pub(super) fn log(db: &Connection, changes: &[Change], upstream: bool) -> rusqlite::Result<()> {
    for c in changes {
        let local: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM changes WHERE kind=?1 AND entity_id=?2 AND upstream=0)",
            params![c.kind, c.id],
            |r| r.get(0),
        )?;
        db.execute(
            "DELETE FROM changes WHERE kind=?1 AND entity_id=?2",
            params![c.kind, c.id],
        )?;
        db.execute(
            "INSERT INTO changes(workspace_id, kind, entity_id, deleted, upstream, clock) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![c.workspace, c.kind, c.id, c.deleted, upstream && !local, c.clock],
        )?;
    }
    Ok(())
}

fn visible(d: &Data, u: &User, workspace: &str) -> bool {
    d.workspaces.iter().any(|w| w.id == workspace)
        && (owns(d, u, workspace)
            || d.folders
                .iter()
                .any(|f| f.workspace_id == workspace && role(d, u, &f.id).is_some()))
}
/// The folder whose role governs an entity.
fn governing_folder(d: &Data, e: &Entity) -> Option<String> {
    match e.kind {
        "folder" => Some(e.id.clone()),
        "note" | "database" => e.fields["folder_id"].as_str().map(String::from),
        "record" => {
            let did = e.fields["database_id"].as_str()?;
            d.databases
                .iter()
                .find(|db| db.id == did)
                .map(|db| db.folder_id.clone())
        }
        _ => None,
    }
}
/// The entry for an entity, or a tombstone when it no longer exists or the user
/// cannot read it.
fn export(d: &Data, u: &User, kind: &str, id: &str, clock: &str) -> Value {
    let tombstone = json!({"kind":kind,"id":id,"deleted":true,"clocks":{"deleted":clock}});
    let Some(e) = current(d, kind, id) else {
        return tombstone;
    };
    let role = match governing_folder(d, &e) {
        Some(folder) => role(d, u, &folder),
        None if owns(d, u, &e.workspace) => Some(Role::Editor),
        None => visible(d, u, &e.workspace).then_some(Role::Viewer),
    };
    let Some(role) = role else {
        return tombstone;
    };
    let mut entry = json!({
        "kind": kind,
        "id": id,
        "fields": e.fields,
        "clocks": d.clocks.get(&key(kind, id)).cloned().unwrap_or_default(),
        "role": role,
    });
    if kind == "note" {
        entry["crdt"] = json!(d.collaboration.get(id));
    }
    entry
}
/// Changes when the user's access to the workspace changes; replicas then resync.
fn access_digest(d: &Data, u: &User, workspace: &str) -> String {
    let mut roles: Vec<String> = d
        .folders
        .iter()
        .filter(|f| f.workspace_id == workspace)
        .filter_map(|f| role(d, u, &f.id).map(|r| format!("{}={r:?}", f.id)))
        .collect();
    roles.sort();
    roles.push(format!("owner={}", owns(d, u, workspace)));
    hash_token(&roles.join(","))
}

pub(super) fn endpoint(
    db: &Connection,
    d: &mut Data,
    u: &User,
    method: &str,
    path: &str,
    query: &str,
    v: Value,
) -> Result<Value, ApiError> {
    match (method, path) {
        ("GET", "sync/changes") => pull(db, d, u, query),
        ("POST", "sync/push") => push(d, u, v),
        _ if d.local && path.starts_with("local/") => local(db, d, u, method, path, v),
        _ => Err(missing()),
    }
}

/// `GET sync/changes?workspace=&since=&limit=&local_only=1`
fn pull(db: &Connection, d: &Data, u: &User, query: &str) -> Result<Value, ApiError> {
    let q: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let workspace = q.get("workspace").ok_or_else(|| bad("Missing workspace"))?;
    let since: i64 = q.get("since").and_then(|s| s.parse().ok()).unwrap_or(0);
    let limit: i64 = q
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(200)
        .clamp(1, 500);
    let local_only = q.get("local_only").map(String::as_str) == Some("1");
    if !visible(d, u, workspace) {
        return Ok(json!({"revoked":true,"cursor":since,"more":false,"changes":[]}));
    }
    let mut statement = db
        .prepare(
            "SELECT seq, kind, entity_id, clock FROM changes
             WHERE workspace_id=?1 AND seq>?2 AND (?3=0 OR upstream=0)
             ORDER BY seq LIMIT ?4",
        )
        .map_err(|_| internal())?;
    let rows: Vec<(i64, String, String, String)> = statement
        .query_map(params![workspace, since, local_only, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .and_then(Iterator::collect)
        .map_err(|_| internal())?;
    let changes: Vec<Value> = rows
        .iter()
        .map(|(seq, kind, id, clock)| {
            let mut entry = export(d, u, kind, id, clock);
            entry["seq"] = json!(seq);
            entry
        })
        .collect();
    Ok(json!({
        "revoked": false,
        "server_id": d.server_id,
        "hlc": d.hlc,
        "access": access_digest(d, u, workspace),
        "cursor": rows.last().map_or(since, |r| r.0),
        "more": rows.len() as i64 == limit,
        "changes": changes,
    }))
}

/// `POST sync/push {workspace_id, upstream?, ops: [{op_id, changed?, ...entry}]}`
fn push(d: &mut Data, u: &User, v: Value) -> Result<Value, ApiError> {
    let workspace = field(&v, "workspace_id")?;
    let upstream = v["upstream"].as_bool() == Some(true);
    // Only a desktop replica applies its upstream server's changes this way.
    if upstream && !d.local {
        return Err(forbidden());
    }
    let ops = v["ops"].as_array().ok_or_else(|| bad("Missing ops"))?;
    if ops.len() > 200 {
        return Err(bad("Push at most 200 operations at a time"));
    }
    if upstream && replica(d, &workspace).is_none() {
        return Err(bad("Subscribe to the workspace first"));
    }
    if !upstream && !visible(d, u, &workspace) {
        return Err(forbidden());
    }
    // Applying the server's own state bypasses the replica's read-only roles.
    d.upstream = upstream;
    d.sync_ops.retain(|_, (at, _)| *at + 7 * 86400 > now());
    let mut results = vec![];
    for op in ops {
        let op_id = op["op_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 120)
            .unwrap_or("");
        let result = match d.sync_ops.get(op_id) {
            Some((_, done)) => done.clone(),
            None if op_id.is_empty() => json!({"status":"invalid","error":"Missing op_id"}),
            None => match apply(d, u, &workspace, op, upstream) {
                Ok(result) => {
                    d.sync_ops.insert(op_id.into(), (now(), result.clone()));
                    result
                }
                Err(ApiError(status, error)) => {
                    let status = match status {
                        StatusCode::FORBIDDEN => "forbidden",
                        StatusCode::NOT_FOUND => "missing",
                        _ => "invalid",
                    };
                    json!({"status":status,"error":error})
                }
            },
        };
        let mut result = result;
        result["op_id"] = json!(op_id);
        results.push(result);
    }
    if upstream {
        // The pull cursor commits in the same transaction as the pulled changes.
        let r = d
            .replicas
            .iter_mut()
            .find(|r| r.workspace_id == workspace)
            .unwrap();
        for op in ops {
            let role: Option<Role> = serde_json::from_value(op["role"].clone()).ok();
            match (op["kind"].as_str(), op["id"].as_str(), role) {
                (Some("workspace"), _, Some(role)) => r.owner = role == Role::Editor,
                (Some("folder"), Some(id), Some(role)) => {
                    r.roles.insert(id.into(), role);
                }
                _ => {}
            }
        }
        if let Some(cursor) = v["cursor"].as_i64() {
            r.pull_cursor = cursor;
        }
        if let Some(access) = v["access"].as_str() {
            r.access = access.into();
        }
    }
    Ok(json!({"results":results,"hlc":d.hlc}))
}

/// Pushed fields newer than the stored clocks, and fields that lost to a
/// different current value.
fn winners(
    d: &Data,
    e: Option<&Entity>,
    k: &str,
    fields: &Fields,
    clocks: &Clocks,
) -> (Fields, Vec<String>) {
    let stored = d.clocks.get(k);
    let mut win = Fields::new();
    let mut lost = vec![];
    for (f, value) in fields {
        let ours = stored.and_then(|c| c.get(f)).map_or("", String::as_str);
        if clocks[f].as_str() > ours {
            win.insert(f.clone(), value.clone());
        } else {
            let current = e.and_then(|e| e.fields.get(f)).unwrap_or(&Value::Null);
            if current != value {
                lost.push(f.clone());
            }
        }
    }
    (win, lost)
}
/// Records the clocks of applied fields. A field whose stored value differs from
/// the pushed one (normalized, or refused, such as a folder move into itself) gets
/// a new clock, so every replica converges on the stored value.
fn settle(d: &mut Data, kind: &str, id: &str, applied: &Fields, clocks: &Clocks) {
    let k = key(kind, id);
    let stored = current(d, kind, id).map(|e| e.fields).unwrap_or_default();
    for (f, value) in applied {
        let clock = if stored.get(f).unwrap_or(&Value::Null) == value {
            clocks[f].clone()
        } else {
            tick(d)
        };
        d.clocks
            .entry(k.clone())
            .or_default()
            .insert(f.clone(), clock);
    }
}
fn outcome(lost: Vec<String>) -> Value {
    if lost.is_empty() {
        json!({"status":"applied"})
    } else {
        json!({"status":"merged","lost":lost})
    }
}
fn text(fields: &Fields, name: &str) -> Result<String, ApiError> {
    field(&json!({ name: fields.get(name) }), name)
}
fn string(fields: &Fields, name: &str) -> Option<String> {
    fields.get(name).and_then(Value::as_str).map(String::from)
}

fn apply(d: &mut Data, u: &User, ws: &str, op: &Value, upstream: bool) -> Result<Value, ApiError> {
    let kind = op["kind"].as_str().unwrap_or("");
    let id = op["id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .ok_or_else(|| bad("Invalid entity ID"))?;
    let fields: Fields = serde_json::from_value(op["fields"].clone()).unwrap_or_default();
    let clocks: Clocks =
        serde_json::from_value(op["clocks"].clone()).map_err(|_| bad("Invalid clocks"))?;
    if fields.len() > 100 || clocks.values().any(|c| !valid_clock(c)) {
        return Err(bad("Invalid fields or clocks"));
    }
    if fields.keys().any(|f| !clocks.contains_key(f)) {
        return Err(bad("Every field needs a clock"));
    }
    for clock in clocks.values() {
        observe(d, clock);
    }
    if op["deleted"].as_bool() == Some(true) {
        let clock = clocks
            .get("deleted")
            .ok_or_else(|| bad("Missing deletion clock"))?;
        return delete(d, u, ws, kind, id, clock, upstream);
    }
    let changed: HashSet<String> =
        serde_json::from_value(op["changed"].clone()).unwrap_or_default();
    let k = key(kind, id);
    let existing = current(d, kind, id);
    match &existing {
        Some(e) if e.workspace != ws => return Err(bad("The entity belongs to another workspace")),
        None if !canonical(id) => return Err(bad("New entities need a UUID")),
        _ => {}
    }
    let result = match kind {
        "workspace" => {
            if id != ws {
                return Err(bad("Invalid workspace"));
            }
            match existing {
                None if upstream => {
                    d.workspaces.push(Workspace {
                        id: id.into(),
                        name: text(&fields, "name")?,
                        owner_id: u.id.clone(),
                        personal: false,
                    });
                    set_special(d, id, &fields);
                    settle(d, kind, id, &fields, &clocks);
                    json!({"status":"applied"})
                }
                None => return Err(forbidden()),
                Some(e) => {
                    if !owns(d, u, id) {
                        return Err(forbidden());
                    }
                    let (win, lost) = winners(d, Some(&e), &k, &fields, &clocks);
                    if win.contains_key("name") {
                        let name = text(&win, "name")?;
                        d.workspaces.iter_mut().find(|w| w.id == id).unwrap().name = name;
                    }
                    set_special(d, id, &win);
                    settle(d, kind, id, &win, &clocks);
                    outcome(lost)
                }
            }
        }
        "folder" => {
            if !owns(d, u, ws) {
                return Err(forbidden());
            }
            match existing {
                None => {
                    let name = text(&fields, "name")?;
                    // A parent that does not exist (yet) places the folder at the top.
                    let parent_id = string(&fields, "parent_id")
                        .filter(|p| d.folders.iter().any(|f| f.id == *p))
                        .map(|p| valid_parent(d, ws, &p, None))
                        .transpose()?;
                    d.folders.push(Folder {
                        id: id.into(),
                        workspace_id: ws.into(),
                        name,
                        parent_id,
                    });
                    settle(d, kind, id, &fields, &clocks);
                    json!({"status":"applied"})
                }
                Some(e) => {
                    let (win, mut lost) = winners(d, Some(&e), &k, &fields, &clocks);
                    let name = win
                        .contains_key("name")
                        .then(|| text(&win, "name"))
                        .transpose()?;
                    // A move that became invalid offline (a cycle, or too deep)
                    // places the folder at the top level instead.
                    let parent = win.get("parent_id").map(|p| {
                        p.as_str()
                            .and_then(|p| valid_parent(d, ws, p, Some(id)).ok())
                    });
                    if matches!(parent, Some(None)) && !win["parent_id"].is_null() {
                        lost.push("parent_id".into());
                    }
                    let f = d.folders.iter_mut().find(|f| f.id == id).unwrap();
                    if let Some(name) = name {
                        f.name = name;
                    }
                    if let Some(parent) = parent {
                        f.parent_id = parent;
                    }
                    settle(d, kind, id, &win, &clocks);
                    outcome(lost)
                }
            }
        }
        "note" => apply_note(d, u, ws, id, &fields, &clocks, op, existing, upstream)?,
        "database" => apply_database(d, u, ws, id, &fields, &clocks, existing)?,
        "record" => apply_record(d, u, ws, id, &fields, &clocks, &changed, existing, upstream)?,
        _ => return Err(bad("Unknown entity kind")),
    };
    Ok(result)
}

fn template(fields: &Fields, upstream: bool) -> Result<String, ApiError> {
    let t = string(fields, "print_template").unwrap_or_default();
    // A replica keeps its server's choice even without that template installed.
    if t.len() > 100
        || (!t.is_empty() && !upstream && plugins::get(plugins::Kind::PdfPrints, &t).is_none())
    {
        return Err(bad("That PDF template is not installed"));
    }
    Ok(t)
}
#[allow(clippy::too_many_arguments)]
fn apply_note(
    d: &mut Data,
    u: &User,
    ws: &str,
    id: &str,
    fields: &Fields,
    clocks: &Clocks,
    op: &Value,
    existing: Option<Entity>,
    upstream: bool,
) -> Result<Value, ApiError> {
    let crdt = op["crdt"].as_str().filter(|s| !s.is_empty());
    let sync_text = |d: &mut Data, update: &str| -> Result<(), ApiError> {
        let n = d.notes.iter().find(|n| n.id == id).ok_or_else(missing)?;
        let actor = collaboration::Actor {
            id: u.id.clone(),
            name: u.name.clone(),
            role: note_role(d, u, n),
            guest: false,
        };
        collaboration::sync(d, &actor, id, json!({ "update": update }))?;
        Ok(())
    };
    let Some(e) = existing else {
        let folder = string(fields, "folder_id").ok_or_else(|| bad("Missing folder"))?;
        if folder_workspace(d, &folder).as_deref() != Some(ws) {
            return Err(bad("Choose a folder in the same workspace"));
        }
        access(d, u, &folder, true)?;
        // The pushed document becomes the note's history; deriving a fresh one
        // from a snapshot would duplicate the text when replicas merge.
        let crdt = crdt.ok_or_else(|| bad("A new note needs its document"))?;
        if crdt.len() > 2_500_000 {
            return Err(bad("Collaborative document reached its storage limit"));
        }
        d.notes.push(Note {
            id: id.into(),
            folder_id: folder,
            title: "Untitled".into(),
            icon: "▤".into(),
            blocks: vec![],
            revision: 1,
            updated_at: now(),
            favorite: false,
            trashed: false,
            cover: String::new(),
            tags: vec![],
            journal: string(fields, "journal").filter(|j| databases::valid_date(j)),
            metadata: normalize_metadata(fields.get("metadata").unwrap_or(&Value::Null))?,
            print_template: template(fields, upstream)?,
        });
        d.collaboration.insert(id.into(), crdt.into());
        if let Err(error) = sync_text(d, "") {
            d.notes.retain(|n| n.id != id);
            d.collaboration.remove(id);
            return Err(error);
        }
        settle(d, "note", id, fields, clocks);
        return Ok(json!({"status":"applied"}));
    };
    let n = d.notes.iter().find(|n| n.id == id).unwrap().clone();
    note_access(d, u, &n, true)?;
    let (mut win, lost) = winners(d, Some(&e), &key("note", id), fields, clocks);
    // The journal date is fixed when the entry is created.
    win.remove("journal");
    let target = match string(&win, "folder_id") {
        Some(target) if target != n.folder_id => {
            access(d, u, &n.folder_id, true)?;
            access(d, u, &target, true)?;
            if folder_workspace(d, &target).as_deref() != Some(ws) {
                return Err(bad("Notes can only move within their workspace"));
            }
            Some(target)
        }
        _ => None,
    };
    let metadata = win.get("metadata").map(normalize_metadata).transpose()?;
    let print_template = win
        .contains_key("print_template")
        .then(|| template(&win, upstream))
        .transpose()?;
    if let Some(update) = crdt {
        sync_text(d, update)?;
    }
    let n = d.notes.iter_mut().find(|n| n.id == id).unwrap();
    if let Some(target) = target {
        n.folder_id = target;
        n.updated_at = now();
    }
    if let Some(metadata) = metadata {
        n.metadata = metadata;
    }
    if let Some(t) = print_template {
        n.print_template = t;
    }
    settle(d, "note", id, &win, clocks);
    Ok(outcome(lost))
}

fn apply_database(
    d: &mut Data,
    u: &User,
    ws: &str,
    id: &str,
    fields: &Fields,
    clocks: &Clocks,
    existing: Option<Entity>,
) -> Result<Value, ApiError> {
    let columns = |fields: &Fields| -> Result<Vec<Column>, ApiError> {
        let columns: Vec<Column> = serde_json::from_value(fields["columns"].clone())
            .map_err(|_| bad("Invalid properties"))?;
        databases::validate_columns(&columns)?;
        Ok(columns)
    };
    let Some(e) = existing else {
        let folder = string(fields, "folder_id").ok_or_else(|| bad("Missing folder"))?;
        if folder_workspace(d, &folder).as_deref() != Some(ws) {
            return Err(bad("Choose a folder in the same workspace"));
        }
        access(d, u, &folder, true)?;
        let db = Database {
            id: id.into(),
            folder_id: folder,
            name: text(fields, "name")?,
            columns: columns(fields)?,
            rows: vec![],
            revision: 1,
            updated_at: now(),
        };
        d.databases.push(db);
        settle(d, "database", id, fields, clocks);
        return Ok(json!({"status":"applied"}));
    };
    let folder = string(&e.fields, "folder_id").unwrap_or_default();
    access(d, u, &folder, true)?;
    let (mut win, lost) = winners(d, Some(&e), &key("database", id), fields, clocks);
    // Databases do not move between folders.
    win.remove("folder_id");
    let name = win
        .contains_key("name")
        .then(|| text(&win, "name"))
        .transpose()?;
    let new_columns = win
        .contains_key("columns")
        .then(|| columns(&win))
        .transpose()?;
    let db = d.databases.iter_mut().find(|db| db.id == id).unwrap();
    if let Some(name) = name {
        db.name = name;
    }
    if let Some(columns) = new_columns {
        // As in a schema edit: values that no longer fit are cleared.
        for row in &mut db.rows {
            row.cells.retain(|key, value| {
                columns
                    .iter()
                    .find(|c| c.id == *key)
                    .is_some_and(|c| databases::cell_valid(c, value))
            });
        }
        db.columns = columns;
    }
    db.revision += 1;
    db.updated_at = now();
    settle(d, "database", id, &win, clocks);
    Ok(outcome(lost))
}

/// The record's cells from pushed fields that fit the schema; others are dropped.
fn cells(columns: &[Column], fields: &Fields) -> BTreeMap<String, Value> {
    fields
        .iter()
        .filter_map(|(f, value)| Some((f.strip_prefix("cells/")?, value)))
        .filter(|(column, value)| {
            !value.is_null()
                && columns
                    .iter()
                    .find(|c| c.id == *column)
                    .is_some_and(|c| databases::cell_valid(c, value))
        })
        .map(|(column, value)| (column.to_string(), value.clone()))
        .collect()
}
#[allow(clippy::too_many_arguments)]
fn apply_record(
    d: &mut Data,
    u: &User,
    ws: &str,
    id: &str,
    fields: &Fields,
    clocks: &Clocks,
    changed: &HashSet<String>,
    existing: Option<Entity>,
    upstream: bool,
) -> Result<Value, ApiError> {
    let did = match &existing {
        Some(e) => string(&e.fields, "database_id"),
        None => string(fields, "database_id"),
    }
    .ok_or_else(|| bad("Missing database"))?;
    let db = d
        .databases
        .iter()
        .find(|db| db.id == did)
        .ok_or_else(missing)?;
    if folder_workspace(d, &db.folder_id).as_deref() != Some(ws) {
        return Err(bad("The database belongs to another workspace"));
    }
    access(d, u, &db.folder_id, true)?;
    let columns = db.columns.clone();
    let Some(e) = existing else {
        if db.rows.len() >= 10000 {
            return Err(bad("Database record limit reached"));
        }
        let row = Record {
            id: id.into(),
            cells: cells(&columns, fields),
            revision: 1,
            updated_at: now(),
            archived: fields
                .get("archived")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            conflict_of: string(fields, "conflict_of"),
        };
        let db = d.databases.iter_mut().find(|db| db.id == did).unwrap();
        db.rows.push(row);
        db.updated_at = now();
        settle(d, "record", id, fields, clocks);
        return Ok(json!({"status":"applied"}));
    };
    let (win, lost) = winners(d, Some(&e), &key("record", id), fields, clocks);
    // Cells this replica changed that lost to a different value are kept in an
    // archived copy, so neither edit is lost.
    let conflicted = !upstream
        && lost
            .iter()
            .any(|f| f.starts_with("cells/") && changed.contains(f));
    let db = d.databases.iter_mut().find(|db| db.id == did).unwrap();
    let row = db.rows.iter_mut().find(|r| r.id == id).unwrap();
    let mut copy = conflicted.then(|| row.cells.clone());
    for (f, value) in &win {
        if let Some(column) = f.strip_prefix("cells/") {
            row.cells.remove(column);
        }
        if let Some(fits) = cells(&columns, &Fields::from([(f.clone(), value.clone())]))
            .into_iter()
            .next()
        {
            row.cells.insert(fits.0, fits.1);
        }
    }
    if let Some(archived) = win.get("archived").and_then(Value::as_bool) {
        row.archived = archived;
    }
    if win.contains_key("conflict_of") {
        row.conflict_of = string(&win, "conflict_of");
    }
    row.revision += 1;
    row.updated_at = now();
    let mut result = outcome(lost);
    if let Some(copy) = copy.as_mut() {
        for (column, value) in cells(&columns, fields) {
            copy.insert(column, value);
        }
        let copy_id = super::id();
        db.rows.push(Record {
            id: copy_id.clone(),
            cells: std::mem::take(copy),
            revision: 1,
            updated_at: now(),
            archived: true,
            conflict_of: Some(id.into()),
        });
        result = json!({"status":"conflict","lost":result["lost"],"copy_id":copy_id});
    }
    db.updated_at = now();
    settle(d, "record", id, &win, clocks);
    Ok(result)
}

/// Deletes an entity unless it changed after the deletion; then it is kept and
/// returned so the replica can restore it.
fn delete(
    d: &mut Data,
    u: &User,
    ws: &str,
    kind: &str,
    id: &str,
    clock: &str,
    upstream: bool,
) -> Result<Value, ApiError> {
    let Some(e) = current(d, kind, id) else {
        return Ok(json!({"status":"applied"}));
    };
    if e.workspace != ws {
        return Err(bad("The entity belongs to another workspace"));
    }
    let deleted_at = parse(clock).0 / 1000;
    let edited_after = |updated_at: u64| updated_at > deleted_at;
    let kept =
        |d: &Data| json!({"status":"conflict","kept":true,"entity":export(d, u, kind, id, clock)});
    match kind {
        // Removing a replica only unsubscribes; a workspace is deleted on the server.
        "workspace" if upstream => {
            removal::route(d, u, &["workspaces", id])?;
        }
        "workspace" => return Err(forbidden()),
        "folder" => {
            let inside: HashSet<String> = d
                .folders
                .iter()
                .filter(|f| ancestors(d, &f.id).any(|a| a == id))
                .map(|f| f.id.clone())
                .collect();
            let newer = |k: String| {
                d.clocks
                    .get(&k)
                    .is_some_and(|c| c.values().any(|c| parse(c).0 / 1000 > deleted_at))
            };
            let changed = d
                .notes
                .iter()
                .any(|n| inside.contains(&n.folder_id) && edited_after(n.updated_at))
                || d.databases
                    .iter()
                    .any(|db| inside.contains(&db.folder_id) && edited_after(db.updated_at))
                || inside.iter().any(|f| newer(key("folder", f)));
            if changed {
                return Ok(kept(d));
            }
            removal::route(d, u, &["folders", id])?;
        }
        "note" => {
            let n = d.notes.iter().find(|n| n.id == id).unwrap();
            if !n.trashed || edited_after(n.updated_at) {
                return Ok(kept(d));
            }
            removal::route(d, u, &["notes", id])?;
        }
        "database" => {
            let db = d.databases.iter().find(|db| db.id == id).unwrap();
            if edited_after(db.updated_at) {
                return Ok(kept(d));
            }
            databases::route(d, u, "DELETE", &["databases", id], json!({}))?;
        }
        "record" => {
            let (db, r) = find_record(d, id).unwrap();
            if edited_after(r.updated_at) {
                return Ok(kept(d));
            }
            let did = db.id.clone();
            databases::route(d, u, "DELETE", &["databases", &did, "rows", id], json!({}))?;
        }
        _ => return Err(bad("Unknown entity kind")),
    }
    Ok(json!({"status":"applied"}))
}

/// A remote workspace replicated into a desktop app's embedded server.
#[derive(Clone, Serialize, Deserialize, Default)]
pub(super) struct Replica {
    pub workspace_id: String,
    /// The desktop connection (see `minerva-client`'s registry) and the server
    /// identity it was subscribed from.
    pub connection_id: String,
    pub server_id: String,
    pub name: String,
    #[serde(default)]
    pub pull_cursor: i64,
    #[serde(default)]
    pub push_cursor: i64,
    /// Fields this replica stamped later than this clock have not been pushed.
    #[serde(default)]
    pub watermark: String,
    #[serde(default)]
    pub access: String,
    /// The user's access on the server: workspace ownership and folder roles.
    #[serde(default)]
    pub owner: bool,
    #[serde(default)]
    pub roles: HashMap<String, Role>,
    #[serde(default)]
    pub revoked: bool,
}
pub(super) fn replica<'a>(d: &'a Data, workspace: &str) -> Option<&'a Replica> {
    if !d.local {
        return None;
    }
    d.replicas.iter().find(|r| r.workspace_id == workspace)
}
/// The role the server grants in a replicated folder, so the replica is read-only
/// where the server would refuse the edit. Subfolders inherit it.
pub(super) fn replica_role(d: &Data, r: &Replica, folder_id: &str) -> Option<Role> {
    if d.upstream {
        return Some(Role::Editor);
    }
    if r.revoked {
        return Some(Role::Viewer);
    }
    ancestors(d, folder_id)
        .find_map(|f| r.roles.get(f).cloned())
        .or_else(|| r.owner.then_some(Role::Editor))
}

/// Endpoints of a desktop app's embedded server, for its sync worker and UI.
fn local(
    db: &Connection,
    d: &mut Data,
    u: &User,
    method: &str,
    path: &str,
    v: Value,
) -> Result<Value, ApiError> {
    admin(u)?;
    let parts: Vec<&str> = path.split('/').collect();
    let pending = |r: &Replica| -> Result<i64, ApiError> {
        db.query_row(
            "SELECT COUNT(*) FROM changes WHERE workspace_id=?1 AND upstream=0 AND seq>?2",
            params![r.workspace_id, r.push_cursor],
            |row| row.get(0),
        )
        .map_err(|_| internal())
    };
    match (method, parts.as_slice()) {
        ("GET", ["local", "replicas"]) => {
            let list = d
                .replicas
                .iter()
                .map(|r| {
                    let mut view = json!(r);
                    view["pending"] = json!(pending(r)?);
                    view["status"] = d
                        .sync_status
                        .get(&r.workspace_id)
                        .cloned()
                        .unwrap_or(json!({}));
                    view["conflicts"] = json!(d
                        .sync_conflicts
                        .iter()
                        .filter(|c| c["workspace_id"] == r.workspace_id.as_str())
                        .count());
                    Ok(view)
                })
                .collect::<Result<Vec<_>, ApiError>>()?;
            Ok(json!(list))
        }
        ("POST", ["local", "replicas"]) => {
            let workspace_id = field(&v, "workspace_id")?;
            if let Some(r) = replica(d, &workspace_id) {
                return Ok(json!(r));
            }
            if d.workspaces.iter().any(|w| w.id == workspace_id) {
                return Err(bad("That workspace already exists here"));
            }
            let r = Replica {
                workspace_id,
                connection_id: field(&v, "connection_id")?,
                server_id: field(&v, "server_id")?,
                name: field(&v, "name")?,
                ..Default::default()
            };
            d.replicas.push(r.clone());
            Ok(json!(r))
        }
        ("PUT", ["local", "replicas", ws]) => {
            let r = d
                .replicas
                .iter_mut()
                .find(|r| r.workspace_id == *ws)
                .ok_or_else(missing)?;
            if let Some(cursor) = v["push_cursor"].as_i64() {
                r.push_cursor = cursor;
            }
            if let Some(cursor) = v["pull_cursor"].as_i64() {
                r.pull_cursor = cursor;
            }
            if let Some(watermark) = v["watermark"].as_str().filter(|c| valid_clock(c)) {
                r.watermark = watermark.into();
            }
            if let Some(revoked) = v["revoked"].as_bool() {
                r.revoked = revoked;
            }
            if v["status"].is_object() {
                d.sync_status.insert(ws.to_string(), v["status"].clone());
            }
            Ok(json!({"ok":true}))
        }
        ("DELETE", ["local", "replicas", ws]) => {
            let r = replica(d, ws).ok_or_else(missing)?;
            let unsent = pending(r)?;
            if unsent > 0 && v["discard"].as_bool() != Some(true) {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    format!("{unsent} changes have not been sent to the server yet"),
                ));
            }
            // Only the local copy goes; the workspace stays on the server.
            d.upstream = true;
            if d.workspaces.iter().any(|w| w.id == *ws) {
                removal::route(d, u, &["workspaces", ws])?;
            }
            d.replicas.retain(|r| r.workspace_id != *ws);
            d.sync_status.remove(*ws);
            d.sync_conflicts.retain(|c| c["workspace_id"] != *ws);
            Ok(json!({"ok":true}))
        }
        ("GET", ["local", "conflicts"]) => Ok(json!(d.sync_conflicts)),
        ("POST", ["local", "conflicts"]) => {
            let items = v["items"].as_array().ok_or_else(|| bad("Missing items"))?;
            for item in items.iter().take(200) {
                let mut item = item.clone();
                item["id"] = json!(super::id());
                item["at"] = json!(now());
                d.sync_conflicts.push(item);
            }
            let excess = d.sync_conflicts.len().saturating_sub(200);
            d.sync_conflicts.drain(..excess);
            Ok(json!({"ok":true}))
        }
        ("DELETE", ["local", "conflicts", id]) => {
            d.sync_conflicts.retain(|c| c["id"] != *id);
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}
