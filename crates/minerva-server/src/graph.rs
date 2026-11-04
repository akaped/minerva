//! The graph of a workspace's notes and the links between them
//! (`GET workspaces/{id}/graph`), for the graph view. Only notes the person can
//! open appear, and only links between two of them: a link to a note they
//! cannot open is left out, so the graph shows nothing the notes would not.
//!
//! Where a person drags a note (or a tag) stays where they put it: their
//! positions are kept per workspace in `graph_positions`
//! (`PUT workspaces/{id}/graph/positions`) and come back with the graph.
use super::*;

/// Positions kept per person and workspace.
const MAX_POSITIONS: usize = 5000;

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS graph_positions(user_id TEXT NOT NULL,workspace_id TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(user_id,workspace_id));",
    )
}

type Positions = std::collections::BTreeMap<String, [f64; 2]>;

fn positions(db: &Connection, user: &str, wid: &str) -> Positions {
    db.query_row(
        "SELECT payload FROM graph_positions WHERE user_id=?1 AND workspace_id=?2",
        params![user, wid],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|raw| serde_json::from_str(&raw).ok())
    .unwrap_or_default()
}

/// `{positions: {id: [x, y] | null}}` moves (or, with `null`, releases) the
/// nodes named; `{reset: true}` releases them all.
pub(super) fn save_positions(
    db: &Connection,
    d: &Data,
    u: &User,
    wid: &str,
    v: &Value,
) -> Result<Value, ApiError> {
    // Only someone who can see the workspace's graph keeps positions in it.
    route(d, u, wid)?;
    let mut kept = if v["reset"] == true {
        Positions::new()
    } else {
        positions(db, &u.id, wid)
    };
    let Some(changes) = v["positions"].as_object() else {
        if v["reset"] == true {
            db.execute(
                "DELETE FROM graph_positions WHERE user_id=?1 AND workspace_id=?2",
                params![u.id, wid],
            )
            .map_err(|_| internal())?;
            return Ok(json!({"positions": {}}));
        }
        return Err(bad("Positions are required"));
    };
    for (id, at) in changes {
        // A note's ID, or a tag node's `#tag`.
        if id.is_empty() || id.len() > 64 {
            return Err(bad("Invalid graph position"));
        }
        if at.is_null() {
            kept.remove(id);
            continue;
        }
        let xy: Vec<f64> = at
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        if xy.len() != 2 || xy.iter().any(|c| !c.is_finite() || c.abs() > 1e6) {
            return Err(bad("Invalid graph position"));
        }
        kept.insert(id.clone(), [xy[0], xy[1]]);
    }
    if kept.len() > MAX_POSITIONS {
        return Err(badv(
            "A graph keeps up to {count} positions",
            &[("count", &MAX_POSITIONS.to_string())],
        ));
    }
    db.execute(
        "INSERT INTO graph_positions(user_id,workspace_id,payload) VALUES(?1,?2,?3) ON CONFLICT(user_id,workspace_id) DO UPDATE SET payload=excluded.payload",
        params![u.id, wid, serde_json::to_string(&kept).map_err(|_| internal())?],
    )
    .map_err(|_| internal())?;
    Ok(json!({"positions": kept}))
}

/// The graph with the positions the person arranged.
pub(super) fn with_positions(
    db: &Connection,
    d: &Data,
    u: &User,
    wid: &str,
) -> Result<Value, ApiError> {
    let mut graph = route(d, u, wid)?;
    graph["positions"] = json!(positions(db, &u.id, wid));
    Ok(graph)
}

/// The IDs of the notes a note links to (`](#note/{id})`), once each.
pub(super) fn links(n: &Note) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for b in &n.blocks {
        let mut rest = b.markdown.as_str();
        while let Some(at) = rest.find("#note/") {
            rest = &rest[at + 6..];
            let id: String = rest.chars().take(36).collect();
            let uuid_like = id.len() == 36
                && id.chars().enumerate().all(|(i, c)| {
                    if matches!(i, 8 | 13 | 18 | 23) {
                        c == '-'
                    } else {
                        c.is_ascii_hexdigit()
                    }
                });
            if uuid_like && id != n.id && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

pub(super) fn route(d: &Data, u: &User, wid: &str) -> Result<Value, ApiError> {
    if !d.workspaces.iter().any(|w| w.id == wid) {
        return Err(missing());
    }
    let folders: HashSet<&str> = d
        .folders
        .iter()
        .filter(|f| f.workspace_id == wid)
        .map(|f| f.id.as_str())
        .collect();
    let notes: Vec<&Arc<Note>> = d
        .notes
        .iter()
        .filter(|n| {
            !n.trashed
                && !n.archived
                && folders.contains(n.folder_id.as_str())
                && note_role(d, u, n).is_some()
        })
        .collect();
    if notes.is_empty() && !folders.iter().any(|f| role(d, u, f).is_some()) {
        return Err(forbidden());
    }
    let shown: HashSet<&str> = notes.iter().map(|n| n.id.as_str()).collect();
    let mut edges = vec![];
    let nodes: Vec<Value> = notes
        .iter()
        .map(|n| {
            for target in links(n) {
                if shown.contains(target.as_str()) {
                    edges.push(json!([n.id, target]));
                }
            }
            let mut tags = n.tags.clone();
            for t in cached_inline_tags(d, n) {
                if !tags.contains(&t) {
                    tags.push(t);
                }
            }
            json!({
                "id": n.id,
                "title": n.title,
                "icon": n.icon,
                "folder_id": n.folder_id,
                "updated_at": n.updated_at,
                "tags": tags,
                "journal": n.journal,
                "record": n.record.is_some(),
            })
        })
        .collect();
    Ok(json!({"nodes": nodes, "links": edges}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_note_links_once() {
        let a = "0b8f5f9e-0000-4000-8000-000000000001";
        let b = "0b8f5f9e-0000-4000-8000-00000000000b";
        let mut n = getting_started("f", "en");
        n.id = a.into();
        n.blocks = vec![
            Block::new(&format!("See [B](#note/{b}) and [B again](#note/{b})."), 0),
            Block::new(
                &format!("Self [A](#note/{a}), broken #note/123, and `#note/` text."),
                1,
            ),
        ];
        assert_eq!(links(&n), vec![b.to_string()]);
    }
}
