use super::*;

const FILE_LIMIT: usize = 10 * 1024 * 1024;
const NOTE_QUOTA: i64 = 100 * 1024 * 1024;
const SERVER_QUOTA: i64 = 1024 * 1024 * 1024;

pub(super) fn initialize(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS attachments (
        id TEXT PRIMARY KEY, note_id TEXT NOT NULL, filename TEXT NOT NULL,
        media_type TEXT NOT NULL, created_at INTEGER NOT NULL, content BLOB NOT NULL
    ); CREATE INDEX IF NOT EXISTS attachments_note ON attachments(note_id);",
    )
}

/// The type of an image or audio file, from its first bytes; anything else is
/// served only as a download.
fn media_type(bytes: &[u8]) -> &'static str {
    let at = |range: std::ops::Range<usize>| bytes.get(range).unwrap_or_default();
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && at(8..12) == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"RIFF") && at(8..12) == b"WAVE" {
        "audio/wav"
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() > 1 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0 && bytes[1] & 0x06 != 0)
    {
        "audio/mpeg"
    } else if bytes.starts_with(b"OggS") {
        "audio/ogg"
    } else if bytes.starts_with(b"fLaC") {
        "audio/flac"
    } else if bytes.starts_with(b"\x1a\x45\xdf\xa3") {
        // WebM, as browsers record audio.
        "audio/webm"
    } else if at(4..8) == b"ftyp" && matches!(at(8..11), b"M4A" | b"mp4" | b"iso" | b"dash") {
        "audio/mp4"
    } else {
        "application/octet-stream"
    }
}

fn filename(raw: &str) -> String {
    let name: String = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '<' | '>' | ':' | '|' | '?' | '*'))
        .take(160)
        .collect();
    let name = name.trim().trim_matches('.');
    if name.is_empty() {
        "attachment".into()
    } else {
        name.into()
    }
}

/// Stores an upload for `note`. Callers have already checked edit permission.
/// Replication passes the attachment's existing ID; storing it again is a no-op.
pub(super) fn upload(
    s: &Store,
    note: &Note,
    uri: &Uri,
    body: &[u8],
    replicated: Option<&str>,
) -> Result<Response, ApiError> {
    if let Some(key) = replicated {
        Uuid::parse_str(key).map_err(|_| bad("Invalid attachment ID"))?;
        let owner: Option<String> =
            s.db.query_row("SELECT note_id FROM attachments WHERE id=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|_| internal())?;
        match owner {
            Some(owner) if owner == note.id => {
                return Ok(axum::Json(json!({"id":key})).into_response())
            }
            Some(_) => return Err(bad("That attachment belongs to another note")),
            None => {}
        }
    } else if note.trashed {
        return Err(bad("Restore this note before adding files"));
    }
    if body.is_empty() || body.len() > FILE_LIMIT {
        return Err(bad("Files must contain 1 byte to 10 MiB"));
    }
    let used: (i64, i64) = s.db.query_row("SELECT COALESCE(SUM(length(content)),0),COALESCE(SUM(CASE WHEN note_id=?1 THEN length(content) ELSE 0 END),0) FROM attachments", [&note.id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|_| internal())?;
    if used.0 + body.len() as i64 > SERVER_QUOTA || used.1 + body.len() as i64 > NOTE_QUOTA {
        return Err(bad(
            "Attachment storage limit reached (100 MiB per note, 1 GiB per server)",
        ));
    }
    let raw = url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
        .find(|(k, _)| k == "filename")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    let name = filename(&raw);
    let key = replicated.map_or_else(id, String::from);
    let media = media_type(body);
    s.db.execute(
        "INSERT INTO attachments VALUES(?1,?2,?3,?4,?5,?6)",
        params![key, note.id, name, media, now(), body],
    )
    .map_err(|_| internal())?;
    Ok((
        StatusCode::CREATED,
        axum::Json(json!({"id":key,"filename":name,"media_type":media,"size":body.len()})),
    )
        .into_response())
}

/// Serves one attachment after `allowed` approves its owning note.
pub(super) fn blob(
    s: &Store,
    key: &str,
    preview: bool,
    allowed: impl FnOnce(&Note) -> Result<(), ApiError>,
) -> Result<Response, ApiError> {
    // Check the parent note's current permissions before loading the potentially large blob.
    let meta: Option<(String, String, String)> =
        s.db.query_row(
            "SELECT note_id,filename,media_type FROM attachments WHERE id=?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|_| internal())?;
    let (note_id, name, media) = meta.ok_or_else(missing)?;
    let note = s
        .data
        .notes
        .iter()
        .find(|n| n.id == note_id)
        .ok_or_else(missing)?;
    allowed(note)?;
    // Images and audio show in the page; everything else downloads.
    if preview && !media.starts_with("image/") && !media.starts_with("audio/") {
        return Err(bad("Only images and audio files play in the page"));
    }
    let bytes: Vec<u8> =
        s.db.query_row("SELECT content FROM attachments WHERE id=?1", [key], |r| {
            r.get(0)
        })
        .map_err(|_| internal())?;
    let mut response = bytes.into_response();
    let h = response.headers_mut();
    h.insert(
        "content-type",
        if preview {
            media.parse().map_err(|_| internal())?
        } else {
            "application/octet-stream".parse().unwrap()
        },
    );
    let encoded: String = url::form_urlencoded::byte_serialize(name.as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    h.insert(
        "content-disposition",
        format!(
            "{}; filename=\"attachment\"; filename*=UTF-8''{}",
            if preview { "inline" } else { "attachment" },
            encoded
        )
        .parse()
        .map_err(|_| internal())?,
    );
    h.insert(
        "cross-origin-resource-policy",
        "same-origin".parse().unwrap(),
    );
    Ok(response)
}

pub(super) async fn endpoint(
    State(store): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    tokio::task::spawn_blocking(move || {
        let s = store.lock().map_err(|_| internal())?;
        let user = authenticate(&s.data, &headers)?;
        let path: Vec<_> = uri.path().trim_matches('/').split('/').collect();
        if path.get(2) == Some(&"notes") {
            let note_id = path[3];
            let note = s.data.notes.iter().find(|n| n.id == note_id).ok_or_else(missing)?;
            note_access(&s.data, &user, note, method != Method::GET)?;
            if method == Method::GET {
                let mut stmt = s.db.prepare("SELECT id,filename,media_type,length(content),created_at FROM attachments WHERE note_id=?1 ORDER BY created_at,id").map_err(|_| internal())?;
                let rows = stmt.query_map([note_id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"filename":r.get::<_,String>(1)?,"media_type":r.get::<_,String>(2)?,"size":r.get::<_,i64>(3)?,"created_at":r.get::<_,i64>(4)?}))).map_err(|_| internal())?;
                let rows = rows.collect::<Result<Vec<_>,_>>().map_err(|_| internal())?;
                return Ok(axum::Json(json!({"attachments":rows})).into_response());
            }
            // PUT stores a replicated attachment under its existing ID.
            let replicated = match (&method, path.get(5)) {
                (&Method::POST, None) => None,
                (&Method::PUT, Some(key)) => Some(*key),
                _ => return Err(ApiError(StatusCode::METHOD_NOT_ALLOWED,"Method not allowed".into())),
            };
            if headers.get("x-minerva-client").and_then(|v| v.to_str().ok()) != Some("1") { return Err(bad("X-Minerva-Client: 1 is required")); }
            return upload(&s, note, &uri, &body, replicated);
        }
        let key = path.get(3).ok_or_else(missing)?;
        blob(&s, key, path.get(4) == Some(&"preview"), |note| note_access(&s.data, &user, note, false))
    }).await.map_err(|_| internal())?
}

/// Unreferenced uploads younger than this stay available for interrupted insertions.
const ORPHAN_GRACE: u64 = 24 * 60 * 60;

/// Attachment IDs mentioned anywhere they could be needed again: current notes,
/// every retained history snapshot, and pinned published pages.
fn referenced(d: &Data) -> HashSet<String> {
    let texts = d
        .notes
        .iter()
        .chain(d.history.values().flatten())
        .flat_map(|n| {
            n.blocks
                .iter()
                .map(|b| b.markdown.as_str())
                .chain(std::iter::once(n.cover.as_str()))
        })
        .chain(sharing::published_text(d));
    let mut ids = HashSet::new();
    for text in texts {
        for token in text.split(|c: char| !(c.is_ascii_hexdigit() || c == '-')) {
            if token.len() == 36 {
                if let Ok(id) = Uuid::parse_str(token) {
                    ids.insert(id.to_string());
                }
            }
        }
    }
    ids
}
struct Stored {
    id: String,
    note_id: String,
    filename: String,
    size: i64,
    created_at: u64,
}
fn stored(s: &Store) -> Result<Vec<Stored>, ApiError> {
    let mut stmt = s.db.prepare("SELECT id,note_id,filename,length(content),created_at FROM attachments ORDER BY created_at").map_err(|_| internal())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Stored {
                id: r.get(0)?,
                note_id: r.get(1)?,
                filename: r.get(2)?,
                size: r.get(3)?,
                created_at: r.get::<_, i64>(4)? as u64,
            })
        })
        .map_err(|_| internal())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| internal())?;
    Ok(rows)
}
fn orphan(file: &Stored, references: &HashSet<String>) -> bool {
    !references.contains(&file.id) && file.created_at + ORPHAN_GRACE < now()
}

/// Administrator storage overview (`GET /api/v1/admin/storage`) and orphan cleanup
/// (`POST /api/v1/admin/storage/cleanup` with `{ids}`).
pub(super) async fn storage(
    State(store): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    tokio::task::spawn_blocking(move || {
        let s = store.lock().map_err(|_| internal())?;
        let user = authenticate(&s.data, &headers)?;
        admin(&user)?;
        let files = stored(&s)?;
        let references = referenced(&s.data);
        let cleanup = uri.path().ends_with("/cleanup");
        if cleanup {
            if method != Method::POST {
                return Err(ApiError(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed".into()));
            }
            if headers.get("x-minerva-client").and_then(|v| v.to_str().ok()) != Some("1") {
                return Err(bad("X-Minerva-Client: 1 is required"));
            }
            let v: Value = serde_json::from_slice(&body).map_err(|_| bad("Invalid JSON"))?;
            let requested: HashSet<&str> = v["ids"].as_array().ok_or_else(|| bad("Choose files to delete"))?.iter().filter_map(|i| i.as_str()).collect();
            // Re-check under the same lock: only files that are still orphaned are deleted.
            let (mut deleted, mut bytes, mut kept) = (0, 0i64, 0);
            for file in files.iter().filter(|f| requested.contains(f.id.as_str())) {
                if orphan(file, &references) {
                    s.db.execute("DELETE FROM attachments WHERE id=?1", [&file.id]).map_err(|_| internal())?;
                    deleted += 1;
                    bytes += file.size;
                } else {
                    kept += 1;
                }
            }
            return Ok(axum::Json(json!({"deleted":deleted,"bytes":bytes,"kept":kept})).into_response());
        }
        if method != Method::GET {
            return Err(ApiError(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed".into()));
        }
        // Sizes are for every note; titles only for notes the administrator may open.
        let title = |nid: &str| {
            s.data.notes.iter().find(|n| n.id == nid).map(|n| {
                let name = if note_role(&s.data, &user, n).is_some() { n.title.clone() } else { "Private note".into() };
                (name, n.trashed)
            })
        };
        let mut per_note: HashMap<&str, (i64, usize)> = HashMap::new();
        for f in &files {
            let entry = per_note.entry(&f.note_id).or_default();
            entry.0 += f.size;
            entry.1 += 1;
        }
        let mut notes: Vec<Value> = per_note
            .into_iter()
            .map(|(nid, (bytes, count))| {
                let (name, trashed) = title(nid).unwrap_or_else(|| ("Unavailable note".into(), true));
                json!({"note_id":nid,"title":name,"trashed":trashed,"bytes":bytes,"files":count})
            })
            .collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n["bytes"].as_i64().unwrap_or(0)));
        let orphans: Vec<Value> = files
            .iter()
            .filter(|f| orphan(f, &references))
            .map(|f| json!({"id":f.id,"filename":f.filename,"size":f.size,"created_at":f.created_at,"note_id":f.note_id,"note_title":title(&f.note_id).map(|t| t.0)}))
            .collect();
        let pending = files.iter().filter(|f| !references.contains(&f.id) && !orphan(f, &references)).count();
        Ok(axum::Json(json!({
            "used": files.iter().map(|f| f.size).sum::<i64>(),
            "files": files.len(),
            "server_quota": SERVER_QUOTA,
            "note_quota": NOTE_QUOTA,
            "file_limit": FILE_LIMIT,
            "grace_seconds": ORPHAN_GRACE,
            "notes": notes,
            "orphans": orphans,
            "recent_unreferenced": pending,
        })).into_response())
    })
    .await
    .map_err(|_| internal())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn recognizes_audio() {
        assert_eq!(super::media_type(b"ID3\x04\0\0"), "audio/mpeg");
        assert_eq!(super::media_type(b"\xff\xfb\x90\x00"), "audio/mpeg");
        assert_eq!(super::media_type(b"RIFF\0\0\0\0WAVEfmt "), "audio/wav");
        assert_eq!(super::media_type(b"OggS\0\x02"), "audio/ogg");
        assert_eq!(super::media_type(b"\x1a\x45\xdf\xa3\x9f"), "audio/webm");
        assert_eq!(super::media_type(b"\0\0\0\x20ftypM4A \0"), "audio/mp4");
        assert_eq!(super::media_type(b"%PDF-1.7"), "application/octet-stream");
    }
}
