//! Permanent deletion of notes, folders, and workspaces.
//!
//! - Notes: folder editors delete notes that are already in the trash.
//! - Folders: folder editors delete a folder with everything in it, including
//!   subfolders.
//! - Workspaces: administrators delete any workspace; owners delete their own when
//!   it is personal, meaning no other member has been given access to it.
//!
//! Everything that belongs to the deleted items goes with them: history, live
//! collaboration state, shares, published pages, databases, sites, grants, and MCP
//! credentials. Attachment blobs are removed when the change is persisted.
use super::*;

fn remove_notes(d: &mut Data, ids: &HashSet<String>) {
    if ids.is_empty() {
        return;
    }
    d.notes.retain(|n| !ids.contains(&n.id));
    d.history.retain(|id, _| !ids.contains(id));
    d.collaboration.retain(|id, _| !ids.contains(id));
    d.presence.retain(|id, _| !ids.contains(id));
    d.shares.retain(|s| !ids.contains(&s.note_id));
    d.publications.retain(|p| !ids.contains(&p.note_id));
    d.journal_templates.retain(|_, note| !ids.contains(note));
}
fn remove_folders(d: &mut Data, ids: &HashSet<String>) {
    let notes: HashSet<String> = d
        .notes
        .iter()
        .filter(|n| ids.contains(&n.folder_id))
        .map(|n| n.id.clone())
        .collect();
    remove_notes(d, &notes);
    d.folders.retain(|f| !ids.contains(&f.id));
    d.databases.retain(|db| !ids.contains(&db.folder_id));
    d.grants.retain(|g| !ids.contains(&g.folder_id));
    d.journal_folders.retain(|_, f| !ids.contains(f));
    d.template_folders.retain(|_, f| !ids.contains(f));
}
/// Whether anyone besides the owner has been given access to the workspace.
pub(super) fn shared(d: &Data, w: &Workspace) -> bool {
    d.workspace_grants
        .iter()
        .any(|g| g.workspace_id == w.id && g.user_id != w.owner_id)
        || d.grants.iter().any(|g| {
            g.user_id != w.owner_id
                && d.folders
                    .iter()
                    .any(|f| f.id == g.folder_id && f.workspace_id == w.id)
        })
}
/// Owners delete their workspaces; nobody else can, administrators included.
pub(super) fn can_delete_workspace(_d: &Data, u: &User, w: &Workspace) -> bool {
    w.owner_id == u.id && !w.personal
}

/// Removes a workspace with everything in it.
pub(super) fn remove_workspace(d: &mut Data, wid: &str) {
    let folders: HashSet<String> = d
        .folders
        .iter()
        .filter(|f| f.workspace_id == wid)
        .map(|f| f.id.clone())
        .collect();
    remove_folders(d, &folders);
    d.sites.retain(|s| s.workspace_id != wid);
    d.workspace_grants.retain(|g| g.workspace_id != wid);
    d.journal_folders.remove(wid);
    d.template_folders.remove(wid);
    d.journal_templates.remove(wid);
    mcp::forget_workspace(d, wid);
    d.workspaces.retain(|w| w.id != wid);
    for a in &mut d.users {
        if a.default_workspace.as_deref() == Some(wid) {
            a.default_workspace = None;
        }
    }
}
/// `DELETE /notes/{id}`, `DELETE /folders/{id}`, and `DELETE /workspaces/{id}`.
pub(super) fn route(d: &mut Data, u: &User, parts: &[&str]) -> Result<Value, ApiError> {
    match parts {
        ["notes", nid] => {
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            // Folder editors only: a note shared for editing cannot be destroyed by
            // its recipient.
            access(d, u, &n.folder_id, true)?;
            if !n.trashed {
                return Err(bad("Move the note to the trash before deleting it"));
            }
            remove_notes(d, &HashSet::from([nid.to_string()]));
            Ok(json!({"ok":true}))
        }
        ["folders", fid] => {
            if !d.folders.iter().any(|f| f.id == *fid) {
                return Err(missing());
            }
            access(d, u, fid, true)?;
            // Subfolders go with their parent.
            let doomed: HashSet<String> = d
                .folders
                .iter()
                .filter(|f| ancestors(d, &f.id).any(|a| a == *fid))
                .map(|f| f.id.clone())
                .collect();
            let notes = d
                .notes
                .iter()
                .filter(|n| doomed.contains(&n.folder_id))
                .count();
            remove_folders(d, &doomed);
            Ok(json!({"ok":true,"deleted_notes":notes}))
        }
        ["workspaces", wid] => {
            let w = d
                .workspaces
                .iter()
                .find(|w| w.id == *wid)
                .ok_or_else(missing)?;
            if !can_delete_workspace(d, u, w) {
                return Err(forbidden());
            }
            remove_workspace(d, wid);
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}
