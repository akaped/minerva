//! The graph of a workspace's notes and the links between them
//! (`GET workspaces/{id}/graph`), for the graph view. Only notes the person can
//! open appear, and only links between two of them: a link to a note they
//! cannot open is left out, so the graph shows nothing the notes would not.
use super::*;

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
            !n.trashed && folders.contains(n.folder_id.as_str()) && note_role(d, u, n).is_some()
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
        let mut n = getting_started("f");
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
