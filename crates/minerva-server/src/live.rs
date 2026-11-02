//! Change signals over a WebSocket (`/api/v1/live`), so open notes sync as soon
//! as someone else edits them or moves around in them, instead of waiting for the
//! next poll. A page sends `{"watch": [note IDs]}`; the server replies
//! `{"note": id}` whenever that note's text, details, or the people in it change,
//! and the page then syncs through the usual HTTP endpoint. Signals carry no
//! content, and each is checked against the reader's current access. Pages that
//! cannot open a socket (the desktop app's own pages, share links) keep polling.
use super::*;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use tokio::sync::broadcast;

/// Notes watched by one socket, at most.
const WATCHED: usize = 32;

pub(super) fn channel() -> broadcast::Sender<String> {
    broadcast::channel(1024).0
}

/// Notes whose text, details, editing state or people differ between `old` and
/// `new`. Unchanged notes are still shared between the two, so this is cheap.
pub(super) fn changed_notes(old: &Data, new: &Data) -> Vec<String> {
    let before: HashMap<&str, &Arc<Note>> = old.notes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut changed: HashSet<&str> = HashSet::new();
    for n in &new.notes {
        if !before
            .get(n.id.as_str())
            .is_some_and(|o| storage::shared(o, n))
        {
            changed.insert(&n.id);
        }
    }
    let now: HashSet<&str> = new.notes.iter().map(|n| n.id.as_str()).collect();
    changed.extend(before.keys().filter(|id| !now.contains(*id)));
    for (id, text) in &new.collaboration {
        if !old
            .collaboration
            .get(id)
            .is_some_and(|o| storage::shared(o, text))
        {
            changed.insert(id);
        }
    }
    let people = |d: &Data, id: &str| -> Vec<(String, String, Option<String>, bool)> {
        let mut v: Vec<_> = d
            .presence
            .get(id)
            .into_iter()
            .flatten()
            .map(collaboration::Presence::identity)
            .collect();
        v.sort();
        v
    };
    for (id, n) in &new.comment_activity {
        if old.comment_activity.get(id) != Some(n) {
            changed.insert(id);
        }
    }
    for id in old.presence.keys().chain(new.presence.keys()) {
        if people(old, id) != people(new, id) {
            changed.insert(id);
        }
    }
    changed.into_iter().map(String::from).collect()
}

/// `GET /api/v1/live`: a WebSocket from a page of this server, signed in.
pub(super) async fn endpoint(
    ws: WebSocketUpgrade,
    State(store): State<Shared>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let origin = headers.get("origin").and_then(|o| o.to_str().ok());
    if origin != Some(sharing::site_url(&headers).as_str()) {
        return Err(forbidden());
    }
    let events = {
        let s = store.lock().map_err(|_| internal())?;
        authenticate(&s.data, &headers)?;
        s.events.subscribe()
    };
    Ok(ws
        .max_message_size(64 * 1024)
        .on_upgrade(move |socket| watch(socket, store, headers, events)))
}

/// Whether the person behind `headers` is still signed in and may read note `id`.
fn readable(store: &Shared, headers: &HeaderMap, id: &str) -> Option<bool> {
    let s = store.lock().ok()?;
    let u = authenticate(&s.data, headers).ok()?;
    Some(
        s.data
            .notes
            .iter()
            .find(|n| n.id == id)
            .is_some_and(|n| note_role(&s.data, &u, n).is_some()),
    )
}

async fn watch(
    mut socket: WebSocket,
    store: Shared,
    headers: HeaderMap,
    mut events: broadcast::Receiver<String>,
) {
    let mut watched: HashSet<String> = HashSet::new();
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        tokio::select! {
            message = socket.recv() => match message {
                Some(Ok(Message::Text(text))) => {
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    let Some(ids) = v["watch"].as_array() else { continue };
                    watched = ids
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|id| id.len() <= 64)
                        .take(WATCHED)
                        .map(String::from)
                        .collect();
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            event = events.recv() => {
                let ids: Vec<String> = match event {
                    Ok(id) if watched.contains(&id) => vec![id],
                    Ok(_) => continue,
                    // Signals were missed: every watched note may have changed.
                    Err(broadcast::error::RecvError::Lagged(_)) => watched.iter().cloned().collect(),
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                for id in ids {
                    match readable(&store, &headers, &id) {
                        None => return,
                        Some(false) => {
                            watched.remove(&id);
                        }
                        Some(true) => {
                            let signal = json!({"note": id}).to_string();
                            if socket.send(Message::Text(signal.into())).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
