//! Yjs-compatible CRDT documents. No client-supplied HTML is trusted.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use yrs::{
    updates::decoder::Decode, Any, Array, Doc, GetString, Map, Out, ReadTxn, StateVector, Text,
    Transact, Update,
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Presence {
    client_id: String,
    user_id: String,
    name: String,
    active_block: Option<String>,
    seen: u64,
}
fn load(d: &Data, note: &Note) -> Result<Doc, ApiError> {
    let doc = Doc::new();
    if let Some(raw) = d.collaboration.get(&note.id) {
        let bytes = STANDARD.decode(raw).map_err(|_| internal())?;
        doc.transact_mut()
            .apply_update(Update::decode_v1(&bytes).map_err(|_| internal())?)
            .map_err(|_| internal())?;
    } else {
        replace(&doc, note);
    }
    Ok(doc)
}
fn replace(doc: &Doc, note: &Note) {
    let title = doc.get_or_insert_text("title");
    let order = doc.get_or_insert_array("order");
    let meta = doc.get_or_insert_map("meta");
    let texts: Vec<_> = note
        .blocks
        .iter()
        .map(|b| doc.get_or_insert_text(format!("block:{}", b.id)))
        .collect();
    let mut txn = doc.transact_mut();
    let len = title.len(&txn);
    title.remove_range(&mut txn, 0, len);
    title.insert(&mut txn, 0, &note.title);
    let len = order.len(&txn);
    order.remove_range(&mut txn, 0, len);
    order.insert_range(&mut txn, 0, note.blocks.iter().map(|b| b.id.as_str()));
    meta.insert(&mut txn, "icon", note.icon.as_str());
    meta.insert(&mut txn, "favorite", note.favorite);
    meta.insert(&mut txn, "trashed", note.trashed);
    meta.insert(&mut txn, "cover", note.cover.as_str());
    meta.insert(
        &mut txn,
        "tags",
        Any::Array(note.tags.iter().map(|t| Any::from(t.as_str())).collect()),
    );
    for (text, block) in texts.iter().zip(&note.blocks) {
        let len = text.len(&txn);
        text.remove_range(&mut txn, 0, len);
        text.insert(&mut txn, 0, &block.markdown);
    }
}
pub(super) fn replace_note(d: &mut Data, note: &Note) -> Result<(), ApiError> {
    let doc = load(d, note)?;
    replace(&doc, note);
    d.collaboration.insert(
        note.id.clone(),
        STANDARD.encode(
            doc.transact()
                .encode_state_as_update_v1(&StateVector::default()),
        ),
    );
    Ok(())
}
/// Gives every note a stored CRDT document, so replicas share one history instead
/// of each deriving its own from the snapshot, which would duplicate text on merge.
pub(super) fn ensure(d: &mut Data) -> Result<(), ApiError> {
    let missing: Vec<Note> = d
        .notes
        .iter()
        .filter(|n| !d.collaboration.contains_key(&n.id))
        .cloned()
        .collect();
    for note in missing {
        replace_note(d, &note)?;
    }
    Ok(())
}
fn project(doc: &Doc, original: &Note) -> Result<Note, ApiError> {
    // Root types are declared before the read transaction, including remote roots.
    let title = doc.get_or_insert_text("title");
    let order = doc.get_or_insert_array("order");
    let meta = doc.get_or_insert_map("meta");
    let txn = doc.transact();
    let mut note = original.clone();
    note.title = title.get_string(&txn);
    if note.title.len() > 200 {
        return Err(bad("Title exceeds 200 bytes"));
    }
    let mut ids = HashSet::new();
    let mut blocks = vec![];
    if order.len(&txn) > 4000 {
        return Err(bad("Too many blocks"));
    }
    for item in order.iter(&txn) {
        let bid = match item {
            Out::Any(Any::String(s)) => s.to_string(),
            _ => return Err(bad("Invalid block order")),
        };
        if bid.is_empty() || bid.len() > 80 {
            return Err(bad("Invalid block ID"));
        }
        // Concurrent reorder can place the same stable ID twice; materialize it once.
        if !ids.insert(bid.clone()) {
            continue;
        }
        let text = txn.get_text(format!("block:{bid}"));
        let markdown = text.map(|t| t.get_string(&txn)).unwrap_or_default();
        if markdown.len() > 100_000 {
            return Err(bad("Block is too large"));
        }
        let mut block = original
            .blocks
            .iter()
            .find(|b| b.id == bid)
            .cloned()
            .unwrap_or_else(|| Block::new("", blocks.len()));
        block.id = bid;
        block.markdown = markdown;
        blocks.push(block);
    }
    if blocks.len() > 2000 {
        return Err(bad("Too many blocks"));
    }
    note.blocks = blocks;
    note.icon = match meta.get(&txn, "icon") {
        Some(Out::Any(Any::String(s))) if s.len() <= 24 => s.to_string(),
        _ => return Err(bad("Invalid icon")),
    };
    note.favorite = match meta.get(&txn, "favorite") {
        Some(Out::Any(Any::Bool(b))) => b,
        _ => return Err(bad("Invalid favorite")),
    };
    note.trashed = match meta.get(&txn, "trashed") {
        Some(Out::Any(Any::Bool(b))) => b,
        _ => return Err(bad("Invalid trash flag")),
    };
    // Cover and tags are optional: documents created before they existed lack them.
    note.cover = match meta.get(&txn, "cover") {
        None => String::new(),
        Some(Out::Any(Any::String(s))) if valid_cover(&s) => s.to_string(),
        _ => return Err(bad("Invalid cover")),
    };
    note.tags = match meta.get(&txn, "tags") {
        None => vec![],
        Some(Out::Any(Any::Array(items))) => normalize_tags(
            &items
                .iter()
                .map(|t| match t {
                    Any::String(s) => Ok(s.to_string()),
                    _ => Err(bad("Invalid tag")),
                })
                .collect::<Result<Vec<_>, _>>()?,
        )?,
        _ => return Err(bad("Invalid tags")),
    };
    Ok(note)
}
/// Who is synchronizing: a signed-in member or a share-link guest. The caller
/// resolves the effective role for this note.
pub(super) struct Actor {
    pub id: String,
    pub name: String,
    pub role: Option<Role>,
    pub guest: bool,
}
pub(super) fn sync(d: &mut Data, actor: &Actor, nid: &str, v: Value) -> Result<Value, ApiError> {
    let original = d
        .notes
        .iter()
        .find(|n| n.id == nid)
        .cloned()
        .ok_or_else(missing)?;
    let raw = v["update"].as_str().unwrap_or("");
    match (&actor.role, raw.is_empty()) {
        (Some(Role::Editor), _) | (Some(Role::Viewer), true) => {}
        _ => return Err(forbidden()),
    }
    let doc = load(d, &original)?;
    if !raw.is_empty() {
        if raw.len() > 1_500_000 {
            return Err(bad("Collaboration update too large"));
        }
        let bytes = STANDARD
            .decode(raw)
            .map_err(|_| bad("Invalid update encoding"))?;
        let update = Update::decode_v1(&bytes).map_err(|_| bad("Invalid CRDT update"))?;
        doc.transact_mut()
            .apply_update(update)
            .map_err(|_| bad("Invalid CRDT update"))?;
    }
    let mut merged = project(&doc, &original)?;
    let encoded = STANDARD.encode(
        doc.transact()
            .encode_state_as_update_v1(&StateVector::default()),
    );
    if encoded.len() > 2_500_000 {
        return Err(bad("Collaborative document reached its storage limit"));
    }
    let changed = merged.title != original.title
        || serde_json::to_value(&merged.blocks).unwrap()
            != serde_json::to_value(&original.blocks).unwrap()
        || merged.favorite != original.favorite
        || merged.trashed != original.trashed
        || merged.icon != original.icon
        || merged.cover != original.cover
        || merged.tags != original.tags;
    if changed {
        let history = d.history.entry(nid.into()).or_default();
        // Keep meaningful checkpoints rather than burning history on every keystroke.
        if history.last().is_none_or(|n| n.updated_at + 30 < now()) {
            history.push(original.clone());
            if history.len() > 50 {
                history.remove(0);
            }
        }
        merged.revision += 1;
        merged.updated_at = now();
        *d.notes.iter_mut().find(|n| n.id == nid).unwrap() = merged.clone();
    }
    d.collaboration.insert(nid.into(), encoded.clone());
    let peers = d.presence.entry(nid.into()).or_default();
    peers.retain(|p| p.seen + 15 > now());
    if let Some(client) = v["client_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() < 100)
    {
        peers.retain(|p| p.client_id != client || p.user_id != actor.id);
        if peers.len() < 128 {
            peers.push(Presence {
                client_id: client.into(),
                user_id: actor.id.clone(),
                name: actor.name.clone(),
                active_block: v["active_block"]
                    .as_str()
                    .filter(|s| s.len() <= 80)
                    .map(String::from),
                seen: now(),
            });
        }
    }
    // Never report stale peers whose access has since been removed.
    let visible: Vec<_> = peers
        .clone()
        .into_iter()
        .filter(|p| sharing::peer_allowed(d, &p.user_id, &original))
        .map(|p| {
            if actor.guest {
                // Guests do not learn member names or IDs.
                let name = if p.user_id.starts_with("share:") { "Guest" } else { "Member" };
                json!({"client_id":p.client_id,"name":name,"active_block":p.active_block,"seen":p.seen})
            } else {
                json!(p)
            }
        })
        .collect();
    let can_edit = actor.role == Some(Role::Editor);
    let note = if actor.guest {
        sharing::guest_view(&merged, actor.role.as_ref().unwrap_or(&Role::Viewer))
    } else {
        let mut view = note_view(&merged);
        view["role"] = json!(actor.role);
        view
    };
    Ok(json!({"update":encoded,"note":note,"peers":visible,"can_edit":can_edit}))
}
