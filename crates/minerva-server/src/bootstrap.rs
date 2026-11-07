//! Bounded sidebar note pages, separate from account/workspace metadata.
use super::*;

pub(super) async fn endpoint(
    State(store): State<Shared>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response, ApiError> {
    tokio::task::spawn_blocking(move || {
        let s = store.lock().map_err(|_| internal())?;
        let u = authenticate(&s.data, &headers)?;
        Ok(axum::Json(notes(&s.data, &u, uri.query().unwrap_or(""))?).into_response())
    })
    .await
    .map_err(|_| internal())?
}

pub(super) fn notes(d: &Data, u: &User, query: &str) -> Result<Value, ApiError> {
    let q: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let workspace = q.get("workspace").ok_or_else(|| bad("Missing workspace"))?;
    if !tokens::within(u, workspace)
        || !(owns(d, u, workspace)
            || d.folders
                .iter()
                .any(|f| f.workspace_id == *workspace && role(d, u, &f.id).is_some()))
    {
        return Err(forbidden());
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 500);
    let after = q.get("after").map(String::as_str).unwrap_or("");
    let readable: HashSet<&str> = d
        .folders
        .iter()
        .filter(|f| f.workspace_id == *workspace && role(d, u, &f.id).is_some())
        .map(|f| f.id.as_str())
        .collect();
    let mut notes: Vec<_> = d
        .notes
        .iter()
        .filter(|n| n.id.as_str() > after && readable.contains(n.folder_id.as_str()))
        .collect();
    notes.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    let more = notes.len() > limit;
    notes.truncate(limit);
    let items: Vec<_> = notes
        .iter()
        .map(|n| {
            let mut v = note_summary(n);
            v["sharing"] = sharing::summary(d, &n.id);
            v["inline_tags"] = json!(cached_inline_tags(d, n));
            v
        })
        .collect();
    Ok(json!({"notes":items,"next":if more {notes.last().map(|n| &n.id)} else {None}}))
}
