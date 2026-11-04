//! Records that open as pages. A record's page is a note in its database's
//! folder, linked back by `Note::record`: the app lists it with the record, not
//! with the folder's notes, and it takes its title from the record's name (the
//! first property), so it is renamed by renaming the record. It is made the first
//! time someone with edit access opens it, and goes when its record or database
//! is deleted.
use super::*;

/// The page of record `rid` of database `did`, if it has one.
pub(super) fn page_of<'a>(d: &'a Data, did: &str, rid: &str) -> Option<&'a Arc<Note>> {
    d.notes.iter().find(|n| {
        n.record
            .as_ref()
            .is_some_and(|r| r.database_id == did && r.record_id == rid)
    })
}
/// The title a record's page shows.
fn title(db: &Database, r: &Record) -> String {
    databases::record_title(db, r).chars().take(200).collect()
}
/// Opens the page of a record, making it (or taking it out of the trash) when
/// needed. Editors only; readers open pages that already exist by their ID.
pub(super) fn open(d: &mut Data, u: &User, did: &str, rid: &str) -> Result<Value, ApiError> {
    let db = d
        .databases
        .iter()
        .find(|db| db.id == did)
        .ok_or_else(missing)?;
    access(d, u, &db.folder_id, true)?;
    let row = db.rows.iter().find(|r| r.id == rid).ok_or_else(missing)?;
    let (folder, name) = (db.folder_id.clone(), title(db, row));
    if let Some(page) = page_of(d, did, rid).cloned() {
        if page.trashed {
            let mut restored = Note::clone(&page);
            restored.trashed = false;
            restored.revision += 1;
            restored.updated_at = now();
            *d.notes.iter_mut().find(|n| n.id == page.id).unwrap() = Arc::new(restored);
        }
        return Ok(json!({"id": page.id}));
    }
    let n = Note {
        id: id(),
        folder_id: folder,
        title: name,
        icon: "▤".into(),
        blocks: vec![Block::new("", 0)],
        revision: 1,
        updated_at: now(),
        favorite: false,
        trashed: false,
        archived: false,
        cover: String::new(),
        tags: vec![],
        journal: None,
        metadata: Default::default(),
        print_template: String::new(),
        record: Some(RecordPage {
            database_id: did.into(),
            record_id: rid.into(),
        }),
        folded: None,
    };
    let result = json!({"id": n.id});
    d.notes.push(Arc::new(n));
    Ok(result)
}
/// After a record changed: its page follows the record's name.
pub(super) fn retitle(d: &mut Data, did: &str, rid: &str) -> Result<(), ApiError> {
    let Some(name) = d
        .databases
        .iter()
        .find(|db| db.id == did)
        .and_then(|db| db.rows.iter().find(|r| r.id == rid).map(|r| title(db, r)))
    else {
        return Ok(());
    };
    let Some(page) = page_of(d, did, rid).filter(|p| p.title != name).cloned() else {
        return Ok(());
    };
    let mut renamed = Note::clone(&page);
    renamed.title = name;
    renamed.revision += 1;
    renamed.updated_at = now();
    *d.notes.iter_mut().find(|n| n.id == page.id).unwrap() = Arc::new(renamed.clone());
    // Open editors show the new title at once.
    collaboration::replace_note(d, &renamed)
}
/// The title a save of note `n` keeps: a record's page keeps its record's name.
pub(super) fn saved_title(d: &Data, n: &Note) -> Option<String> {
    let link = n.record.as_ref()?;
    let db = d.databases.iter().find(|db| db.id == link.database_id)?;
    db.rows
        .iter()
        .find(|r| r.id == link.record_id)
        .map(|r| title(db, r))
}
/// Pages of records that no longer exist in database `did` (or of all its
/// records, when the database is gone).
pub(super) fn orphans(d: &Data, did: &str) -> HashSet<String> {
    let db = d.databases.iter().find(|db| db.id == did);
    d.notes
        .iter()
        .filter(|n| {
            n.record.as_ref().is_some_and(|r| {
                r.database_id == did
                    && db.is_none_or(|db| !db.rows.iter().any(|x| x.id == r.record_id))
            })
        })
        .map(|n| n.id.clone())
        .collect()
}
