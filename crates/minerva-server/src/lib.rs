mod administration;
mod ai;
mod anki;
mod attachments;
mod collaboration;
mod databases;
mod export;
mod flashcards;
mod journal;
mod mail;
mod mcp;
mod plugins;
mod removal;
mod render;
mod replication;
mod server_plugins;
mod sharing;
mod sites;
mod twofactor;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use minerva_core::*;
use render::render;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct Account {
    user: User,
    password_hash: String,
    #[serde(default)]
    two_factor: twofactor::TwoFactor,
    /// Set when an administrator chooses the password: the person picks their
    /// own on their next sign-in.
    #[serde(default)]
    must_change_password: bool,
    /// The workspace the app opens first.
    #[serde(default)]
    default_workspace: Option<String>,
    /// The overview page's widgets, in order (see `web/dashboard.js`); `None`
    /// shows the default layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dashboard: Option<Value>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Session {
    user_id: String,
    expires: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct WorkspaceGrant {
    workspace_id: String,
    user_id: String,
    role: Role,
}
#[derive(Clone, Serialize, Deserialize, Default)]
struct Data {
    server_id: String,
    users: Vec<Account>,
    workspaces: Vec<Workspace>,
    folders: Vec<Folder>,
    grants: Vec<Grant>,
    /// Access to every folder of a workspace, including folders added later.
    #[serde(default)]
    workspace_grants: Vec<WorkspaceGrant>,
    notes: Vec<Note>,
    history: HashMap<String, Vec<Note>>,
    sessions: HashMap<String, Session>,
    #[serde(default)]
    collaboration: HashMap<String, String>,
    #[serde(skip)]
    presence: HashMap<String, Vec<collaboration::Presence>>,
    /// Pending second-factor sign-ins, by challenge hash (memory only).
    #[serde(skip)]
    login_challenges: HashMap<String, twofactor::Challenge>,
    #[serde(default)]
    databases: Vec<Database>,
    #[serde(default)]
    mcp_credentials: Vec<mcp::Credential>,
    #[serde(default)]
    shares: Vec<sharing::Share>,
    #[serde(default)]
    publications: Vec<sharing::Publication>,
    #[serde(default)]
    sites: Vec<sites::Site>,
    /// Workspace ID → its Journal folder, Templates folder, and journal template note.
    #[serde(default)]
    journal_folders: HashMap<String, String>,
    #[serde(default)]
    template_folders: HashMap<String, String>,
    #[serde(default)]
    journal_templates: HashMap<String, String>,
    /// Replication: per-entity field clocks, the last issued clock, and applied
    /// push operations (see `replication.rs`).
    #[serde(default)]
    clocks: HashMap<String, std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    hlc: String,
    #[serde(default)]
    sync_ops: HashMap<String, (u64, Value)>,
    /// Set while applying changes pulled from an upstream server, so they are not
    /// pushed back (memory only).
    #[serde(skip)]
    upstream: bool,
    /// A desktop app's embedded server, which may apply upstream changes.
    #[serde(default)]
    local: bool,
    /// Remote workspaces replicated into a desktop app, their sync status (memory
    /// only), and offline edits that lost to other changes.
    #[serde(default)]
    replicas: Vec<replication::Replica>,
    #[serde(skip)]
    sync_status: HashMap<String, Value>,
    #[serde(default)]
    sync_conflicts: Vec<Value>,
    /// Email waiting to be sent, `/remindme` reminders, and password-reset links
    /// by token hash (see `mail.rs`).
    #[serde(default)]
    mail_outbox: Vec<mail::Outgoing>,
    #[serde(default)]
    reminders: Vec<mail::Reminder>,
    #[serde(default)]
    password_resets: HashMap<String, mail::Reset>,
}
pub struct Store {
    db: Connection,
    data: Data,
    attempts: Limiter,
    ai: ai::Runtime,
    plugin_tasks: HashMap<String, tokio::task::AbortHandle>,
}
pub type Shared = Arc<Mutex<Store>>;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
fn password_hash(password: &str) -> Result<String, ApiError> {
    if password.len() < 12 || password.len() > 256 {
        return Err(bad("Passwords must contain 12–256 characters"));
    }
    Argon2::default()
        .hash_password(
            password.as_bytes(),
            &SaltString::encode_b64(Uuid::new_v4().as_bytes()).map_err(|_| internal())?,
        )
        .map(|v| v.to_string())
        .map_err(|_| internal())
}
impl Store {
    pub fn open(
        path: &str,
        email: &str,
        password: &str,
    ) -> Result<Shared, Box<dyn std::error::Error>> {
        Self::open_with(path, || initial(email, password, false))
    }
    /// The embedded server of a desktop app: one owner who never signs in with a
    /// password, and a new session for this process each time it opens. Nothing
    /// listens on the network; the app passes requests to `app` in-process.
    pub fn open_local(path: &str) -> Result<(Shared, String), Box<dyn std::error::Error>> {
        let unusable = format!("{}{}", id(), id());
        let store = Self::open_with(path, || initial("local@minerva.localhost", &unusable, true))?;
        let token = format!("{}{}", id(), id());
        {
            let mut s = store.lock().map_err(|_| "Store lock poisoned")?;
            if !s.data.local {
                return Err(format!("{path} belongs to a server, not a desktop app").into());
            }
            let mut d = s.data.clone();
            let user_id = d.users[0].user.id.clone();
            d.sessions.clear();
            d.sessions.insert(
                hash_token(&token),
                Session {
                    user_id,
                    expires: now() + 86400 * 365 * 10,
                },
            );
            persist(&mut s, d).map_err(|e| e.1)?;
        }
        Ok((store, token))
    }
    fn open_with(
        path: &str,
        initial: impl FnOnce() -> Result<Data, ApiError>,
    ) -> Result<Shared, Box<dyn std::error::Error>> {
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS state (id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT NOT NULL);")?;
        attachments::initialize(&db)?;
        replication::initialize(&db)?;
        let ai = ai::initialize(&db, path)?;
        server_plugins::initialize(&db)?;
        flashcards::initialize(&db)?;
        let raw: Option<String> = db
            .query_row("SELECT payload FROM state WHERE id=1", [], |r| r.get(0))
            .optional()?;
        let mut data = match raw {
            Some(raw) => serde_json::from_str(&raw)?,
            None => initial().map_err(|e| e.1)?,
        };
        ensure_personal(&mut data);
        // Databases from before replication get a change-log row and field clocks
        // for every entity, so a replica's first pull receives everything.
        if replication::is_empty(&db)? {
            let changes = replication::track(&Data::default(), &mut data).map_err(|e| e.1)?;
            replication::log(&db, &changes, false)?;
        }
        db.execute(
            "INSERT OR REPLACE INTO state VALUES(1, ?1)",
            [serde_json::to_string(&data)?],
        )?;
        Ok(Arc::new(Mutex::new(Self {
            db,
            data,
            attempts: Limiter::default(),
            ai,
            plugin_tasks: HashMap::new(),
        })))
    }
}
/// A dashboard layout: up to 40 widgets of known types and sizes, with small
/// option objects; `null` restores the default layout.
fn valid_dashboard(v: &Value) -> Result<Option<Value>, ApiError> {
    if v.is_null() {
        return Ok(None);
    }
    let widgets = v.as_array().ok_or_else(|| bad("Widgets must be a list"))?;
    if widgets.len() > 40 || v.to_string().len() > 32 * 1024 {
        return Err(bad("A dashboard holds up to 40 widgets"));
    }
    const TYPES: [&str; 8] = [
        "welcome",
        "folders",
        "recent",
        "mentions",
        "favorites",
        "journal",
        "flashcards",
        "chart",
    ];
    for w in widgets {
        let ok = w["id"]
            .as_str()
            .is_some_and(|i| !i.is_empty() && i.len() <= 40)
            && w["type"].as_str().is_some_and(|t| TYPES.contains(&t))
            && w["size"]
                .as_str()
                .is_some_and(|s| matches!(s, "s" | "m" | "l" | "full"))
            && (w.get("options").is_none() || w["options"].is_object());
        if !ok {
            return Err(bad("Invalid widget"));
        }
    }
    Ok(Some(v.clone()))
}
/// The name of every account's personal workspace.
const PERSONAL: &str = "My Workspace";
/// Creates an account's personal workspace, with a first folder, and makes it
/// the workspace the app opens.
fn create_personal(d: &mut Data, user_id: &str) -> String {
    let w = Workspace {
        id: id(),
        name: PERSONAL.into(),
        owner_id: user_id.into(),
        personal: true,
    };
    let folder = Folder {
        id: id(),
        workspace_id: w.id.clone(),
        name: "General".into(),
        parent_id: None,
    };
    d.notes.push(getting_started(&folder.id));
    d.folders.push(folder);
    if let Some(a) = d.users.iter_mut().find(|a| a.user.id == user_id) {
        a.default_workspace = Some(w.id.clone());
    }
    let wid = w.id.clone();
    d.workspaces.push(w);
    wid
}
/// The manual every personal workspace starts with: one block per paragraph of
/// `getting-started.md` (its code, tables and lists have no blank lines).
fn getting_started(folder_id: &str) -> Note {
    Note {
        id: id(),
        folder_id: folder_id.into(),
        title: "Getting Started".into(),
        icon: "🧭".into(),
        blocks: include_str!("getting-started.md")
            .split("\n\n")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .enumerate()
            .map(|(i, s)| Block::new(s, i))
            .collect(),
        revision: 1,
        updated_at: now(),
        favorite: true,
        trashed: false,
        cover: "gradient:3".into(),
        tags: vec!["guide".into()],
        journal: None,
        metadata: Default::default(),
        print_template: String::new(),
    }
}
/// Every account has one personal workspace. Older data adopts an unshared
/// workspace the account owns that is evidently personal; otherwise one is
/// created.
fn ensure_personal(d: &mut Data) {
    for a in &mut d.users {
        if !d.local && a.user.admin {
            a.default_workspace = None;
            a.user.can_create_workspace = false;
        } else {
            a.user.can_create_workspace = true;
        }
    }
    let users: Vec<String> = d
        .users
        .iter()
        .filter(|a| d.local || !a.user.admin)
        .map(|a| a.user.id.clone())
        .collect();
    for uid in users {
        if d.workspaces.iter().any(|w| w.owner_id == uid && w.personal) {
            continue;
        }
        let adopt = d
            .workspaces
            .iter()
            .filter(|w| w.owner_id == uid && replication::replica(d, &w.id).is_none())
            .filter(|w| {
                matches!(
                    w.name.as_str(),
                    "My workspace" | "My Workspace" | "Personal"
                )
            })
            .find(|w| !removal::shared(d, w))
            .map(|w| w.id.clone());
        match adopt {
            Some(wid) => {
                let w = d.workspaces.iter_mut().find(|w| w.id == wid).unwrap();
                w.personal = true;
                w.name = PERSONAL.into();
                if let Some(a) = d.users.iter_mut().find(|a| a.user.id == uid) {
                    a.default_workspace.get_or_insert(wid);
                }
            }
            None => {
                let had_default = d
                    .users
                    .iter()
                    .find(|a| a.user.id == uid)
                    .and_then(|a| a.default_workspace.clone());
                create_personal(d, &uid);
                // Someone who already works elsewhere keeps opening there.
                if let Some(default) = had_default {
                    if let Some(a) = d.users.iter_mut().find(|a| a.user.id == uid) {
                        a.default_workspace = Some(default);
                    }
                }
            }
        }
    }
}
/// A new server's data: its first account, a workspace, and a welcome note.
fn initial(email: &str, password: &str, local: bool) -> Result<Data, ApiError> {
    let user = User {
        id: id(),
        name: if local { "You" } else { "Administrator" }.into(),
        email: email.to_lowercase(),
        admin: true,
        can_create_workspace: local,
        disabled: false,
    };
    // A server starts with a team workspace to share (the administrator's own
    // "My Workspace" is added when the server opens); the desktop app's own
    // server has only the personal one.
    let workspace = Workspace {
        id: id(),
        name: if local { PERSONAL } else { "Team" }.into(),
        owner_id: user.id.clone(),
        personal: local,
    };
    let folder = Folder {
        id: id(),
        workspace_id: workspace.id.clone(),
        name: "Getting started".into(),
        parent_id: None,
    };
    let text = ["## A little space for big ideas", "Welcome to **Minerva**, your team's home for notes, knowledge, and everything in between.", "> Your ideas belong to you. All of your notes are stored on your own Rust server.", "## Make yourself at home", "- [ ] Create a folder for your next project", "- [ ] Write your first note", "- [ ] Invite someone to your workspace", "## Think in blocks", "Click any block to edit its Markdown. Press **Enter** to close the line, render it, and start a new block.", "Use the **+** button to add a heading, task, quote, or code block. Your changes sync live with everyone viewing this note.", "---", "Small thoughts. Meaningful connections. A place to begin."];
    let note = Note {
        id: id(),
        folder_id: folder.id.clone(),
        title: "Welcome to Minerva".into(),
        icon: "✳".into(),
        blocks: text
            .iter()
            .enumerate()
            .map(|(i, s)| Block::new(s, i))
            .collect(),
        revision: 1,
        updated_at: now(),
        favorite: true,
        trashed: false,
        cover: String::new(),
        tags: vec![],
        journal: None,
        metadata: Default::default(),
        print_template: String::new(),
    };
    Ok(Data {
        server_id: id(),
        users: vec![Account {
            user,
            password_hash: password_hash(password)?,
            two_factor: Default::default(),
            must_change_password: false,
            default_workspace: None,
            dashboard: None,
        }],
        workspaces: if local { vec![workspace] } else { vec![] },
        folders: if local { vec![folder] } else { vec![] },
        notes: if local { vec![note] } else { vec![] },
        local,
        ..Default::default()
    })
}
#[derive(Debug)]
struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, axum::Json(json!({"error":self.1}))).into_response()
    }
}
fn bad(s: &str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, s.into())
}
fn forbidden() -> ApiError {
    ApiError(
        StatusCode::FORBIDDEN,
        "You do not have permission for this action".into(),
    )
}
fn missing() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "Not found".into())
}
fn internal() -> ApiError {
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Storage operation failed".into(),
    )
}
fn field(v: &Value, key: &str) -> Result<String, ApiError> {
    let s = v[key]
        .as_str()
        .ok_or_else(|| bad(&format!("Missing {key}")))?
        .trim();
    if s.is_empty() || s.len() > 200 {
        return Err(bad("Names must contain 1–200 characters"));
    }
    Ok(s.into())
}
fn role(d: &Data, u: &User, folder_id: &str) -> Option<Role> {
    if u.admin && !d.local {
        return None;
    }
    let f = d.folders.iter().find(|f| f.id == folder_id)?;
    if let Some(r) = replication::replica(d, &f.workspace_id) {
        return replication::replica_role(d, r, folder_id);
    }
    // Administrators have no access to other people's workspaces: like
    // everyone, they see what they own and what is shared with them.
    if d.workspaces
        .iter()
        .any(|w| w.id == f.workspace_id && w.owner_id == u.id)
    {
        return Some(Role::Editor);
    }
    // The higher of workspace access and a folder grant.
    let workspace = d
        .workspace_grants
        .iter()
        .find(|g| g.workspace_id == f.workspace_id && g.user_id == u.id)
        .map(|g| g.role.clone());
    // A folder grant also covers the folder's subfolders.
    let folder = ancestors(d, folder_id)
        .filter_map(|fid| {
            d.grants
                .iter()
                .find(|g| g.folder_id == fid && g.user_id == u.id)
                .map(|g| g.role.clone())
        })
        .max_by_key(|r| *r == Role::Editor);
    match (workspace, folder) {
        (Some(Role::Editor), _) | (_, Some(Role::Editor)) => Some(Role::Editor),
        (w, f) => w.or(f),
    }
}
/// The folder and its parents, innermost first. Depth is bounded so damaged data
/// cannot loop.
fn ancestors<'a>(d: &'a Data, folder_id: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    std::iter::successors(Some(folder_id), move |id| {
        d.folders
            .iter()
            .find(|f| f.id == *id)
            .and_then(|f| f.parent_id.as_deref())
    })
    .take(32)
}
/// Checks a new parent for a folder: same workspace, and not the folder itself or
/// one of its descendants.
fn valid_parent(
    d: &Data,
    workspace_id: &str,
    parent: &str,
    moving: Option<&str>,
) -> Result<String, ApiError> {
    let p = d
        .folders
        .iter()
        .find(|f| f.id == parent)
        .ok_or_else(missing)?;
    if p.workspace_id != workspace_id {
        return Err(bad("Choose a folder in the same workspace"));
    }
    if let Some(moving) = moving {
        if ancestors(d, parent).any(|id| id == moving) {
            return Err(bad("A folder cannot move into itself"));
        }
    }
    if ancestors(d, parent).count() >= 8 {
        return Err(bad("Folders can be nested up to 8 levels"));
    }
    Ok(parent.to_string())
}
/// Normalizes a metadata key: `Client Name` becomes `client_name`.
pub(crate) fn metadata_key(raw: &str) -> String {
    let mut key = String::new();
    for c in raw.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            key.push(c);
        } else if !key.is_empty() && !key.ends_with('_') {
            key.push('_');
        }
    }
    key.trim_end_matches('_').chars().take(40).collect()
}
/// Up to 50 `{key: value}` pairs; keys are normalized, values are plain text.
fn normalize_metadata(v: &Value) -> Result<std::collections::BTreeMap<String, String>, ApiError> {
    let Some(fields) = v.as_object() else {
        return if v.is_null() {
            Ok(Default::default())
        } else {
            Err(bad("Metadata must be an object"))
        };
    };
    if fields.len() > 50 {
        return Err(bad("A note can have up to 50 metadata fields"));
    }
    let mut out = std::collections::BTreeMap::new();
    for (raw, value) in fields {
        let key = metadata_key(raw);
        if key.is_empty() || !key.starts_with(|c: char| c.is_ascii_lowercase()) {
            return Err(bad(&format!(
                "“{raw}” is not a valid field name; start with a letter"
            )));
        }
        let value = match value {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => continue,
            _ => return Err(bad("Metadata values must be text")),
        };
        if value.chars().count() > 4000 {
            return Err(bad(&format!("“{key}” is too long")));
        }
        if !value.is_empty() {
            out.insert(key, value);
        }
    }
    Ok(out)
}
fn access(d: &Data, u: &User, f: &str, write: bool) -> Result<(), ApiError> {
    match role(d, u, f) {
        Some(Role::Editor) => Ok(()),
        Some(Role::Viewer) if !write => Ok(()),
        _ => Err(forbidden()),
    }
}
/// Effective access to one note: its folder role, raised by a direct member share.
fn note_role(d: &Data, u: &User, n: &Note) -> Option<Role> {
    if u.admin && !d.local {
        return None;
    }
    let folder = role(d, u, &n.folder_id);
    match (folder, sharing::member_role(d, &u.id, n)) {
        (_, Some(Role::Editor)) => Some(Role::Editor),
        (Some(r), _) => Some(r),
        (None, shared) => shared,
    }
}
fn note_access(d: &Data, u: &User, n: &Note, write: bool) -> Result<(), ApiError> {
    match note_role(d, u, n) {
        Some(Role::Editor) => Ok(()),
        Some(Role::Viewer) if !write => Ok(()),
        _ => Err(forbidden()),
    }
}
/// Covers are a preset gradient, an uploaded attachment, or an HTTPS image URL.
fn valid_cover(cover: &str) -> bool {
    cover.is_empty()
        || cover
            .strip_prefix("gradient:")
            .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8"))
        || render::attachment(cover).is_some_and(|(_, absolute)| !absolute)
        || (cover.len() <= 500
            && url::Url::parse(cover)
                .is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some())
            && !cover.contains(['"', '<', '>', ' ', '\'']))
}
/// Up to 20 unique normalized tags.
fn normalize_tags(tags: &[String]) -> Result<Vec<String>, ApiError> {
    if tags.len() > 20 {
        return Err(bad("A note can have up to 20 tags"));
    }
    let mut normalized: Vec<String> = vec![];
    for tag in tags {
        let tag = render::normalize_tag(tag)
            .ok_or_else(|| bad("Tags use letters, digits, -, _ and /, up to 40 characters"))?;
        if !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }
    Ok(normalized)
}
fn inline_tags(n: &Note) -> Vec<String> {
    let mut tags: Vec<String> = vec![];
    for b in &n.blocks {
        for tag in render::inline_tags(&b.markdown) {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    tags
}
/// Commits a modified snapshot of the data to SQLite when it changed, with its
/// change-log rows.
fn persist(s: &mut Store, mut d: Data) -> Result<(), ApiError> {
    let upstream = std::mem::take(&mut d.upstream);
    let changes = replication::track(&s.data, &mut d)?;
    let payload = serde_json::to_string(&d).map_err(|_| internal())?;
    if payload != serde_json::to_string(&s.data).map_err(|_| internal())? {
        // Attachments of permanently deleted notes go in the same transaction.
        let kept: HashSet<&str> = d.notes.iter().map(|n| n.id.as_str()).collect();
        let removed: Vec<&str> = s
            .data
            .notes
            .iter()
            .map(|n| n.id.as_str())
            .filter(|id| !kept.contains(id))
            .collect();
        let tx = s.db.unchecked_transaction().map_err(|_| internal())?;
        tx.execute("UPDATE state SET payload=?1 WHERE id=1", params![payload])
            .map_err(|_| internal())?;
        for id in removed {
            tx.execute("DELETE FROM attachments WHERE note_id=?1", [id])
                .map_err(|_| internal())?;
        }
        replication::log(&tx, &changes, upstream).map_err(|_| internal())?;
        // A deleted account's private data goes with it.
        let users: HashSet<&str> = d.users.iter().map(|a| a.user.id.as_str()).collect();
        for gone in s
            .data
            .users
            .iter()
            .filter(|a| !users.contains(a.user.id.as_str()))
        {
            for table in [
                "ai_chats",
                "flashcard_decks",
                "flashcard_media",
                "plugin_jobs",
            ] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE user_id=?1"),
                    [&gone.user.id],
                )
                .map_err(|_| internal())?;
            }
        }
        tx.commit().map_err(|_| internal())?;
    }
    s.data = d;
    Ok(())
}
fn admin(u: &User) -> Result<(), ApiError> {
    if u.admin {
        Ok(())
    } else {
        Err(forbidden())
    }
}
fn owns(d: &Data, u: &User, w: &str) -> bool {
    if u.admin && !d.local {
        return false;
    }
    if let Some(r) = replication::replica(d, w) {
        return d.upstream || (r.owner && !r.revoked);
    }
    d.workspaces.iter().any(|v| v.id == w && v.owner_id == u.id)
}
/// The owner, and members who can edit the whole workspace: they add folders
/// and set up its Journal and Templates.
fn edits_workspace(d: &Data, u: &User, w: &str) -> bool {
    if u.admin && !d.local {
        return false;
    }
    owns(d, u, w)
        || (replication::replica(d, w).is_none()
            && d.workspace_grants
                .iter()
                .any(|g| g.workspace_id == w && g.user_id == u.id && g.role == Role::Editor))
}
fn authenticate(d: &Data, headers: &HeaderMap) -> Result<User, ApiError> {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let cookie = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|p| p.trim().strip_prefix("minerva_session="))
        });
    let session = bearer
        .or(cookie)
        .and_then(|t| d.sessions.get(&hash_token(t)))
        .filter(|s| s.expires > now());
    session
        .and_then(|s| {
            d.users
                .iter()
                .find(|a| a.user.id == s.user_id && !a.user.disabled)
        })
        .map(|a| a.user.clone())
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "Please sign in".into()))
}
/// The session token presented as a bearer token or session cookie.
fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("cookie")
                .and_then(|h| h.to_str().ok())
                .and_then(|h| {
                    h.split(';')
                        .find_map(|p| p.trim().strip_prefix("minerva_session="))
                })
        })
}
/// Creates a seven-day session and the cookie that carries it.
fn start_session(d: &mut Data, user: User, secure: &str) -> (Value, Option<String>) {
    let token = format!("{}{}", id(), id());
    d.sessions.retain(|_, s| s.expires > now());
    d.sessions.insert(
        hash_token(&token),
        Session {
            user_id: user.id.clone(),
            expires: now() + 86400 * 7,
        },
    );
    (
        json!({"user":user,"token":token}),
        Some(format!(
            "minerva_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{secure}"
        )),
    )
}
fn note_view(n: &Note) -> Value {
    let mut v = serde_json::to_value(n).unwrap();
    v["inline_tags"] = json!(inline_tags(n));
    v["rendered"] = json!(n
        .blocks
        .iter()
        .map(|b| render(&b.markdown))
        .collect::<Vec<_>>());
    v
}

/// Only the reduced editor may be embedded, and only by explicitly trusted origins.
async fn editor_pane() -> Response {
    let origins = std::env::var("MINERVA_EMBED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|raw| {
            let url = url::Url::parse(raw.trim()).ok()?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return None;
            }
            Some(url.origin().ascii_serialization())
        })
        .collect::<Vec<_>>()
        .join(" ");
    let policy = format!("default-src 'self'; script-src 'self' https://www.youtube.com https://s.ytimg.com; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: http: https:; media-src 'self' blob: http: https:; connect-src 'self'; frame-src https://www.youtube.com https://www.youtube-nocookie.com 'self'; frame-ancestors 'self' {origins}; base-uri 'none'; form-action 'self'");
    (
        [("content-security-policy", policy)],
        axum::response::Html(include_str!("../../../web/index.html")),
    )
        .into_response()
}

pub fn app(store: Shared) -> Router {
    mail::spawn(store.clone());
    Router::new()
        .route("/", get(|| async { axum::response::Html(include_str!("../../../web/index.html")) }))
        .route("/pane", get(editor_pane))
        .route("/app.js", get(|| async { ([("content-type","text/javascript; charset=utf-8")], include_str!("../../../web/dist/app.js")) }))
        .route("/style.css", get(|| async { ([("content-type","text/css; charset=utf-8")], include_str!("../../../web/style.css")) }))
        .route("/share", get(|| async { ([("x-robots-tag","noindex"),("referrer-policy","no-referrer")], axum::response::Html(include_str!("../../../web/index.html"))) }))
        .route("/mermaid.js", get(|| async { ([("content-type","text/javascript; charset=utf-8"),("cache-control","public, max-age=86400")], include_str!("../../../web/dist/mermaid.js")) }))
        .route("/export/{note_id}", get(export::export))
        .route("/export.js", get(|| async { ([("content-type","text/javascript; charset=utf-8")], export::EXPORT_JS) }))
        .route("/favicon.ico", get(|| async { ([("content-type","image/x-icon"),("cache-control","public, max-age=86400")], include_bytes!("../../../web/icons/favicon.ico").as_slice()) }))
        .route("/favicon.png", get(|| async { ([("content-type","image/png"),("cache-control","public, max-age=86400")], include_bytes!("../../../web/icons/favicon-32.png").as_slice()) }))
        .route("/apple-touch-icon.png", get(|| async { ([("content-type","image/png"),("cache-control","public, max-age=86400")], include_bytes!("../../../web/icons/apple-touch-icon.png").as_slice()) }))
        .route("/plugins/{*path}", get(plugins::serve))
        .route("/public.css", get(|| async { ([("content-type","text/css; charset=utf-8")], sharing::PUBLIC_CSS) }))
        .route("/shared", get(sharing::public))
        .route("/shared/", get(sharing::public))
        .route("/shared/{*path}", get(sharing::public))
        .route("/p", get(sharing::moved))
        .route("/p/", get(sharing::moved))
        .route("/p/{*path}", get(sharing::moved))
        .route("/preview/{note_id}", get(sharing::preview))
        .route("/api/v1/shared/{*path}", any(sharing::shared).layer(DefaultBodyLimit::max(10 * 1024 * 1024)))
        .route("/api/v1/admin/overview", get(administration::overview))
        .route("/api/v1/admin/storage", any(attachments::storage))
        .route("/api/v1/admin/storage/cleanup", any(attachments::storage))
        .route("/api/v1/notes/{note_id}/attachments", any(attachments::endpoint).layer(DefaultBodyLimit::max(10 * 1024 * 1024)))
        .route("/api/v1/notes/{note_id}/attachments/{attachment_id}", any(attachments::endpoint).layer(DefaultBodyLimit::max(10 * 1024 * 1024)))
        .route("/api/v1/attachments/{attachment_id}", get(attachments::endpoint))
        .route("/api/v1/attachments/{attachment_id}/preview", get(attachments::endpoint))
        // Anki packages can be large; their media is not kept.
        .route("/api/v1/flashcards/import", any(flashcards::import).layer(DefaultBodyLimit::max(100 * 1024 * 1024)))
        .route("/api/v1/flashcards/{*path}", any(flashcards::endpoint))
        .route("/api/v1/ai/{*path}", any(ai::endpoint))
        .route("/api/v1/server-plugins/{*path}", any(server_plugins::endpoint))
        .route("/api/v1/{*path}", any(api))
        .route("/api/workspaces/{workspace_id}/mcp", any(mcp::endpoint))
        // Public sites (`/blog/...`); paths the server uses are reserved and win.
        .route("/{site}", get(sharing::public))
        .route("/{site}/", get(sharing::public))
        .route("/{site}/{*path}", get(sharing::public))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(axum::middleware::map_response(|mut r: Response| async move {
            let h = r.headers_mut();
            h.insert("x-content-type-options", "nosniff".parse().unwrap());
            h.insert("referrer-policy", "same-origin".parse().unwrap());
            h.entry("content-security-policy").or_insert("default-src 'self'; script-src 'self' https://www.youtube.com https://s.ytimg.com; style-src 'self' 'unsafe-inline'; img-src 'self' data: http: https:; media-src 'self' blob: http: https:; connect-src 'self'; frame-src 'self' https: http:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
            h.entry("cache-control").or_insert("no-store".parse().unwrap()); r
        }))
        .layer(axum::middleware::from_fn_with_state(store.clone(), administration::restrict))
        .with_state(store)
}
async fn api(
    State(store): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    tokio::task::spawn_blocking(move || {
        let mut s = store.lock().map_err(|_| internal())?;
        let mut d = s.data.clone();
        let write = method != Method::GET;
        if write
            && headers
                .get("x-minerva-client")
                .and_then(|h| h.to_str().ok())
                != Some("1")
        {
            return Err(bad("X-Minerva-Client: 1 is required"));
        }
        let value: Value = if body.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(&body).map_err(|_| bad("Invalid JSON"))?
        };
        let path = uri.path().trim_start_matches("/api/v1/");
        let outcome = if path.starts_with("sync/") || path.starts_with("local/") {
            authenticate(&d, &headers).and_then(|u| {
                let query = uri.query().unwrap_or("");
                replication::endpoint(&s.db, &mut d, &u, method.as_str(), path, query, value)
                    .map(|v| (v, None))
            })
        } else {
            dispatch(
                &mut d,
                &mut s.attempts,
                method.as_str(),
                path,
                &headers,
                value,
            )
        };
        // Failed second-factor attempts must still count, even though the
        // request's other changes are discarded.
        s.data.login_challenges = d.login_challenges.clone();
        let (result, cookie) = outcome?;
        if write {
            let actor = authenticate(&d, &headers).ok();
            mail::assignments(&s.data, &mut d, actor.as_ref());
            persist(&mut s, d)?;
        }
        let mut response = axum::Json(result).into_response();
        if let Some(cookie) = cookie {
            response
                .headers_mut()
                .insert("set-cookie", cookie.parse().map_err(|_| internal())?);
        }
        Ok(response)
    })
    .await
    .map_err(|_| internal())?
}
/// Sign-in throttling per account, so one attacker cannot lock everyone out, plus
/// a generous server-wide ceiling against credential stuffing.
#[derive(Default)]
struct Limiter {
    global: Vec<u64>,
    accounts: HashMap<String, Vec<u64>>,
}
impl Limiter {
    const PER_ACCOUNT: usize = 10;
    const SERVER: usize = 300;
    fn check(&mut self, key: &str) -> Result<(), ApiError> {
        let at = now();
        self.global.retain(|t| *t + 60 > at);
        self.accounts.retain(|_, times| {
            times.retain(|t| *t + 60 > at);
            !times.is_empty()
        });
        let account = self.accounts.get(key).map_or(0, Vec::len);
        if account >= Self::PER_ACCOUNT || self.global.len() >= Self::SERVER {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many sign-in attempts. Try again in a minute.".into(),
            ));
        }
        self.global.push(at);
        self.accounts.entry(key.to_string()).or_default().push(at);
        Ok(())
    }
}
/// A real Argon2 hash to verify against when the account does not exist, so
/// response time does not reveal which email addresses have accounts.
fn dummy_hash() -> &'static str {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| password_hash("not-a-real-account-password").unwrap_or_default())
}
fn dispatch(
    d: &mut Data,
    attempts: &mut Limiter,
    method: &str,
    path: &str,
    headers: &HeaderMap,
    v: Value,
) -> Result<(Value, Option<String>), ApiError> {
    let secure = if std::env::var("MINERVA_SECURE_COOKIE").as_deref() == Ok("1") {
        "; Secure"
    } else {
        ""
    };
    if path == "health" && method == "GET" {
        return Ok((
            json!({"status":"ok","api_version":1,"server_id":d.server_id,"sync_protocol":1,"mode":if d.local {"local"} else {"server"},"mail":mail::enabled()}),
            None,
        ));
    }
    if path == "login" && method == "POST" {
        let email = field(&v, "email")?.to_lowercase();
        attempts.check(&format!("login:{email}"))?;
        let password = v["password"]
            .as_str()
            .filter(|s| s.len() <= 256)
            .ok_or_else(|| bad("Invalid password"))?;
        let Some(a) = d
            .users
            .iter()
            .find(|a| a.user.email == email && !a.user.disabled)
        else {
            if let Ok(hash) = PasswordHash::new(dummy_hash()) {
                let _ = Argon2::default().verify_password(password.as_bytes(), &hash);
            }
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "Invalid email or password".into(),
            ));
        };
        if Argon2::default()
            .verify_password(
                password.as_bytes(),
                &PasswordHash::new(&a.password_hash).map_err(|_| internal())?,
            )
            .is_err()
        {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "Invalid email or password".into(),
            ));
        }
        let user = a.user.clone();
        // With 2FA, a correct password only earns a challenge for the second step.
        if let Some(challenge) = twofactor::challenge(d, &user.id) {
            return Ok((challenge, None));
        }
        return Ok(start_session(d, user, secure));
    }
    if path == "login/2fa" && method == "POST" {
        let challenge = v["challenge"].as_str().unwrap_or("");
        attempts.check(&format!("2fa:{}", hash_token(challenge)))?;
        let user = twofactor::complete(d, &v)?;
        return Ok(start_session(d, user, secure));
    }
    if path.starts_with("password-reset") && method == "POST" {
        attempts.check(&format!("reset:{}", v["email"].as_str().unwrap_or("token")))?;
        if let Some(result) = mail::reset_route(d, path, &v)? {
            return Ok((result, None));
        }
    }
    let u = authenticate(d, headers)?;
    let parts: Vec<&str> = path.split('/').collect();
    if let Some(result) = administration::route(d, &u, method, &parts, &v)? {
        return Ok((result, None));
    }

    if method == "GET" {
        if let Some(result) = mail::people_route(d, &u, &parts)? {
            return Ok((result, None));
        }
    }
    if parts.first() == Some(&"reminders") {
        return Ok((mail::reminders_route(d, &u, method, &parts, &v)?, None));
    }
    if parts.first() == Some(&"databases") {
        return Ok((databases::route(d, &u, method, &parts, v)?, None));
    }
    if method == "DELETE"
        && matches!(parts.as_slice(), [_, _])
        && matches!(parts[0], "notes" | "folders" | "workspaces")
    {
        return Ok((removal::route(d, &u, &parts)?, None));
    }
    if parts.first() == Some(&"sites") {
        return Ok((sites::route(d, &u, method, &parts, &v)?, None));
    }
    if parts.first() == Some(&"mcp-credentials") {
        return Ok((mcp::credentials(d, &u, method, &parts, v)?, None));
    }
    if parts.first() == Some(&"account") {
        let current = session_token(headers).map(String::from);
        if let Some(result) = twofactor::route(d, &u, method, &parts, &v, current)? {
            return Ok((result, None));
        }
    }
    if matches!(parts.first(), Some(&"journal" | &"templates")) || parts.get(2) == Some(&"template")
    {
        if let Some(result) = journal::route(d, &u, method, &parts, &v)? {
            return Ok((result, None));
        }
    }
    if parts.first() == Some(&"sharing") || matches!(parts.get(2), Some(&"shares" | &"publication"))
    {
        if let Some(result) = sharing::route(d, &u, method, &parts, &v)? {
            return Ok((result, None));
        }
    }
    let result = match (method, parts.as_slice()) {
        ("POST", ["notes", nid, "sync"]) => {
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            let actor = collaboration::Actor {
                id: u.id.clone(),
                name: u.name.clone(),
                role: note_role(d, &u, n),
                guest: false,
            };
            collaboration::sync(d, &actor, nid, v)?
        }
        ("POST", ["logout"]) => {
            let token = session_token(headers);
            if let Some(t) = token {
                d.sessions.remove(&hash_token(t));
            }
            return Ok((
                json!({"ok":true}),
                Some(format!(
                    "minerva_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{secure}"
                )),
            ));
        }
        ("GET", ["bootstrap"]) => {
            let folders: Vec<_> = d
                .folders
                .iter()
                .filter(|f| role(d, &u, &f.id).is_some())
                .collect();
            let workspaces: Vec<_> = d
                .workspaces
                .iter()
                .filter(|w| {
                    owns(d, &u, &w.id)
                        || (!u.admin
                            && d.workspace_grants
                                .iter()
                                .any(|g| g.workspace_id == w.id && g.user_id == u.id))
                        || folders.iter().any(|f| f.workspace_id == w.id)
                })
                .collect();
            let notes: Vec<_> = d
                .notes
                .iter()
                .filter(|n| role(d, &u, &n.folder_id).is_some())
                .map(|n| {
                    let mut v = serde_json::to_value(n).unwrap();
                    v.as_object_mut().unwrap().remove("blocks");
                    v["sharing"] = sharing::summary(d, &n.id);
                    v["inline_tags"] = json!(inline_tags(n));
                    v
                })
                .collect();
            let visible = |map: &HashMap<String, String>| -> HashMap<String, String> {
                map.iter()
                    .filter(|(ws, _)| workspaces.iter().any(|w| &&w.id == ws))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            };
            let roles: HashMap<_, _> = folders
                .iter()
                .map(|f| (&f.id, role(d, &u, &f.id)))
                .collect();
            let workspaces_view: Vec<Value> = workspaces
                .iter()
                .map(|w| {
                    let mut v = json!(w);
                    v["can_delete"] = json!(removal::can_delete_workspace(d, &u, w));
                    v["can_edit"] = json!(edits_workspace(d, &u, &w.id));
                    v["shared"] = json!(removal::shared(d, w));
                    v
                })
                .collect();
            let account = d.users.iter().find(|a| a.user.id == u.id);
            json!({"server_id":d.server_id,"user":u,"must_change_password":account.is_some_and(|a| a.must_change_password),"owns_workspace":d.workspaces.iter().any(|w| owns(d,&u,&w.id)),"default_workspace":account.and_then(|a| a.default_workspace.clone()),"workspaces":workspaces_view,"folders":folders,"notes":notes,"shared_notes":sharing::shared_with(d,&u),"journal_folders":visible(&d.journal_folders),"template_folders":visible(&d.template_folders),"journal_templates":visible(&d.journal_templates),"sites":sites::listed(d,&workspaces),"roles":roles,"databases":d.databases.iter().filter(|db|role(d,&u,&db.folder_id).is_some()).map(|db|json!({"id":db.id,"folder_id":db.folder_id,"name":db.name,"updated_at":db.updated_at,"record_count":db.rows.iter().filter(|r|!r.archived).count()})).collect::<Vec<_>>()})
        }
        ("POST", ["search"]) => {
            let query = v["query"].as_str().unwrap_or("").trim().to_lowercase();
            if query.len() > 200 {
                return Err(bad("Search query is too long"));
            }
            // `#tag` searches tags (note tags and inline tags) by prefix.
            let tag = query.strip_prefix('#').map(str::to_string);
            let matches: Vec<_> = d.notes.iter()
                .filter(|n| !n.trashed && note_role(d, &u, n).is_some())
                .filter(|n| match &tag {
                    Some(tag) => n.tags.iter().chain(inline_tags(n).iter()).any(|t| t.starts_with(tag.as_str())),
                    None => n.title.to_lowercase().contains(&query) || n.blocks.iter().any(|b| b.markdown.to_lowercase().contains(&query)),
                })
                .take(50).map(|n| json!({"id": n.id, "title": n.title, "icon": n.icon, "folder_id": n.folder_id})).collect();
            json!(matches)
        }
        ("POST", ["workspaces"]) => {
            if u.admin && !d.local {
                return Err(forbidden());
            }
            let w = Workspace {
                id: id(),
                name: field(&v, "name")?,
                owner_id: u.id.clone(),
                personal: false,
            };
            d.folders.push(Folder {
                id: id(),
                workspace_id: w.id.clone(),
                name: "General".into(),
                parent_id: None,
            });
            d.workspaces.push(w.clone());
            json!(w)
        }
        // Renames a workspace you own; personal workspaces keep their name.
        ("PATCH", ["workspaces", wid]) => {
            if !owns(d, &u, wid) {
                return Err(forbidden());
            }
            let name = field(&v, "name")?;
            let w = d
                .workspaces
                .iter_mut()
                .find(|w| w.id == *wid)
                .ok_or_else(missing)?;
            if w.personal {
                return Err(bad("A personal workspace keeps its name"));
            }
            w.name = name;
            json!(w)
        }
        ("POST", ["folders"]) => {
            let wid = field(&v, "workspace_id")?;
            if !d.workspaces.iter().any(|w| w.id == wid) {
                return Err(missing());
            }
            if !edits_workspace(d, &u, &wid) {
                return Err(forbidden());
            }
            let parent_id = match v["parent_id"].as_str().filter(|p| !p.is_empty()) {
                Some(parent) => Some(valid_parent(d, &wid, parent, None)?),
                None => None,
            };
            let f = Folder {
                id: id(),
                workspace_id: wid,
                name: field(&v, "name")?,
                parent_id,
            };
            d.folders.push(f.clone());
            json!(f)
        }
        ("PATCH", ["folders", fid]) => {
            let f = d
                .folders
                .iter()
                .find(|f| f.id == *fid)
                .ok_or_else(missing)?;
            if !edits_workspace(d, &u, &f.workspace_id) {
                return Err(forbidden());
            }
            let name = v.get("name").map(|_| field(&v, "name")).transpose()?;
            // `parent_id`: a folder to move into, or null for the top level.
            let parent = match v.get("parent_id") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(p) => Some(Some(valid_parent(
                    d,
                    &f.workspace_id,
                    p.as_str().ok_or_else(|| bad("Invalid parent folder"))?,
                    Some(fid),
                )?)),
            };
            let f = d.folders.iter_mut().find(|f| f.id == *fid).unwrap();
            if let Some(name) = name {
                f.name = name;
            }
            if let Some(parent) = parent {
                f.parent_id = parent;
            }
            json!(f)
        }
        ("PATCH", ["notes", nid]) => {
            // Moves a note to another folder of the same workspace. Folder editors
            // only, on both ends; a share recipient cannot move a note away.
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            access(d, &u, &n.folder_id, true)?;
            let target = field(&v, "folder_id")?;
            access(d, &u, &target, true)?;
            let workspace = |id: &str| {
                d.folders
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.workspace_id.clone())
            };
            if workspace(&n.folder_id) != workspace(&target) {
                return Err(bad("Notes can only move within their workspace"));
            }
            let n = d.notes.iter_mut().find(|n| n.id == *nid).unwrap();
            n.folder_id = target;
            n.updated_at = now();
            let mut view = note_view(n);
            view["role"] = json!(note_role(
                d,
                &u,
                d.notes.iter().find(|n| n.id == *nid).unwrap()
            ));
            view
        }
        ("POST", ["notes"]) => {
            let fid = field(&v, "folder_id")?;
            access(d, &u, &fid, true)?;
            let n = Note {
                id: id(),
                folder_id: fid,
                title: field(&v, "title")?,
                icon: "▤".into(),
                blocks: vec![Block::new("", 0)],
                revision: 1,
                updated_at: now(),
                favorite: false,
                trashed: false,
                cover: String::new(),
                tags: vec![],
                journal: None,
                metadata: Default::default(),
                print_template: String::new(),
            };
            d.notes.push(n.clone());
            note_view(&n)
        }
        ("GET", ["notes", nid]) => {
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            note_access(d, &u, n, false)?;
            let mut view = note_view(n);
            view["role"] = json!(note_role(d, &u, n));
            view["sharing"] = sharing::summary(d, &n.id);
            let target = format!("#note/{}", n.id);
            view["backlinks"] = json!(d
                .notes
                .iter()
                .filter(|o| o.id != n.id && !o.trashed && note_role(d, &u, o).is_some())
                .filter(|o| o.blocks.iter().any(|b| b.markdown.contains(&target)))
                .map(|o| json!({"id":o.id,"title":o.title,"icon":o.icon}))
                .collect::<Vec<_>>());
            view
        }
        ("PUT", ["notes", nid]) => {
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            note_access(d, &u, n, true)?;
            let save: SaveNote = serde_json::from_value(v).map_err(|_| bad("Invalid note"))?;
            if save.revision != n.revision {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "This note changed on another client. Export your draft before reloading."
                        .into(),
                ));
            }
            if save.title.trim().is_empty()
                || save.title.len() > 200
                || save.icon.len() > 24
                || save.blocks.len() > 2000
            {
                return Err(bad("Invalid title, icon, or too many blocks"));
            }
            let mut ids = HashSet::new();
            for b in &save.blocks {
                if b.markdown.len() > 100_000
                    || b.id.len() > 80
                    || b.id.is_empty()
                    || !ids.insert(&b.id)
                    || !b.x.is_finite()
                    || !b.y.is_finite()
                    || b.x.abs() > 100_000.0
                    || b.y.abs() > 100_000.0
                {
                    return Err(bad("Invalid block"));
                }
            }
            if !valid_cover(&save.cover) {
                return Err(bad("Invalid cover"));
            }
            let tags = normalize_tags(&save.tags)?;
            let history = d.history.entry(n.id.clone()).or_default();
            history.push(n.clone());
            if history.len() > 50 {
                history.remove(0);
            }
            let n = d.notes.iter_mut().find(|n| n.id == *nid).unwrap();
            n.title = save.title;
            n.icon = save.icon;
            n.blocks = save.blocks;
            n.favorite = save.favorite;
            n.trashed = save.trashed;
            n.cover = save.cover;
            n.tags = tags;
            n.revision += 1;
            n.updated_at = now();
            let saved = n.clone();
            collaboration::replace_note(d, &saved)?;
            let mut view = note_view(&saved);
            view["role"] = json!(note_role(d, &u, &saved));
            view
        }
        ("PUT", ["notes", nid, "metadata"]) => {
            // Metadata is saved on its own, so editing a note's text never
            // overwrites it (and the reverse).
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            note_access(d, &u, n, true)?;
            let metadata = normalize_metadata(&v["metadata"])?;
            let template = v["print_template"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_string();
            if !template.is_empty() && plugins::get(plugins::Kind::PdfPrints, &template).is_none() {
                return Err(bad("That PDF template is not installed"));
            }
            let n = d.notes.iter_mut().find(|n| n.id == *nid).unwrap();
            n.metadata = metadata;
            n.print_template = template;
            json!({"metadata": n.metadata, "print_template": n.print_template})
        }
        ("GET", ["notes", nid, "history"]) => {
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            // Past revisions may hold text removed before the note was shared, so
            // they stay with folder members rather than individual share recipients.
            access(d, &u, &n.folder_id, false)?;
            json!(d.history.get(*nid).cloned().unwrap_or_default())
        }
        ("POST", ["render"]) => {
            // One block, or a list of blocks (an AI reply) in one request.
            if let Some(blocks) = v["blocks"].as_array() {
                if blocks.len() > 500 {
                    return Err(bad("Too many blocks"));
                }
                let html = blocks
                    .iter()
                    .map(|b| match b.as_str() {
                        Some(md) if md.len() <= 100_000 => Ok(render(md)),
                        _ => Err(bad("Invalid block")),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok((json!({ "html": html }), None));
            }
            let md = v["markdown"]
                .as_str()
                .ok_or_else(|| bad("Missing markdown"))?;
            if md.len() > 100_000 {
                return Err(bad("Block is too large"));
            }
            json!({"html":render(md)})
        }
        ("GET", ["admin"]) => {
            admin(&u)?;
            let users: Vec<Value> = d
                .users
                .iter()
                .map(|a| {
                    let mut user = json!(a.user);
                    user["two_factor"] = json!(a.two_factor.enabled());
                    user
                })
                .collect();
            json!({"users":users,"grants":[],"workspace_grants":[]})
        }
        ("POST", ["users"]) => {
            admin(&u)?;
            let email = field(&v, "email")?.to_lowercase();
            if !email.contains('@') {
                return Err(bad("Invalid email"));
            }
            if d.users.iter().any(|a| a.user.email == email) {
                return Err(bad("Email already exists"));
            }
            // With email, the password may be left out: the invitation lets the
            // person choose their own.
            let password = match v["password"].as_str().filter(|p| !p.is_empty()) {
                Some(p) => p.to_string(),
                None if mail::enabled() => format!("{}{}", id(), id()),
                None => String::new(),
            };
            let hash = password_hash(&password)?;
            let user = User {
                id: id(),
                name: field(&v, "name")?,
                email,
                admin: false,
                can_create_workspace: true,
                disabled: false,
            };
            d.users.push(Account {
                user: user.clone(),
                password_hash: hash,
                two_factor: Default::default(),
                must_change_password: true,
                default_workspace: None,
                dashboard: None,
            });
            create_personal(d, &user.id);
            let invited = mail::invite(d, &u, &user);
            let mut out = json!(user);
            out["invited"] = json!(invited);
            out
        }
        ("PATCH", ["users", uid]) => {
            admin(&u)?;
            if *uid == u.id {
                return Err(bad("You cannot modify your own administrator account here"));
            }
            let a = d
                .users
                .iter_mut()
                .find(|a| a.user.id == *uid)
                .ok_or_else(missing)?;
            if v.get("can_create_workspace").is_some() {
                a.user.can_create_workspace = !a.user.admin;
            }
            if v.get("name").is_some() {
                a.user.name = field(&v, "name")?;
            }
            if let Some(b) = v["admin"].as_bool() {
                a.user.admin = b;
                a.user.can_create_workspace = !b;
            }
            if v.get("email").is_some() {
                let email = field(&v, "email")?.to_lowercase();
                if !email.contains('@') {
                    return Err(bad("Invalid email"));
                }
                if email != a.user.email && d.users.iter().any(|o| o.user.email == email) {
                    return Err(bad("Email already exists"));
                }
                let a = d.users.iter_mut().find(|a| a.user.id == *uid).unwrap();
                a.user.email = email;
            }
            let a = d.users.iter_mut().find(|a| a.user.id == *uid).unwrap();
            if let Some(b) = v["disabled"].as_bool() {
                a.user.disabled = b;
            }
            if let Some(p) = v["password"].as_str() {
                a.password_hash = password_hash(p)?;
                a.must_change_password = true;
            }
            if v["two_factor"].as_bool() == Some(false) {
                twofactor::reset(a);
            }
            let user = json!(a.user);
            d.sessions.retain(|_, s| s.user_id != *uid);
            // Half-finished second-factor sign-ins do not survive account changes.
            twofactor::forget(d, uid);
            user
        }
        // Deletes an account. Workspaces it owns are kept and pass to the
        // administrator; its private data (AI chats, flashcards, plugin jobs) is
        // removed when the change is saved.
        ("DELETE", ["users", uid]) => {
            admin(&u)?;
            if *uid == u.id {
                return Err(bad("You cannot delete your own account"));
            }
            if !d.users.iter().any(|a| a.user.id == *uid) {
                return Err(missing());
            }
            // Personal workspaces go with the account: nobody else may open them.
            // Workspaces shared with others pass to the administrator, so their
            // members keep them.
            let owned: Vec<(String, bool)> = d
                .workspaces
                .iter()
                .filter(|w| w.owner_id == *uid)
                .map(|w| (w.id.clone(), removal::shared(d, w)))
                .collect();
            let (mut transferred, mut removed) = (0, 0);
            for (wid, shared) in owned {
                if shared {
                    let successor = v["successor_id"].as_str().filter(|next| *next != *uid && d.users.iter().any(|a| a.user.id==*next && !a.user.admin && !a.user.disabled)).ok_or_else(||bad("Choose a non-administrator successor for shared workspaces before deleting this account"))?.to_string();
                    if let Some(w) = d.workspaces.iter_mut().find(|w| w.id == wid) {
                        w.owner_id = successor.clone();
                        w.personal = false;
                    }
                    d.workspace_grants
                        .retain(|g| g.workspace_id != wid || g.user_id != successor);
                    transferred += 1;
                } else {
                    removal::remove_workspace(d, &wid);
                    removed += 1;
                }
            }
            d.users.retain(|a| a.user.id != *uid);
            d.sessions.retain(|_, s| s.user_id != *uid);
            d.grants.retain(|g| g.user_id != *uid);
            d.workspace_grants.retain(|g| g.user_id != *uid);
            d.shares.retain(|s| s.user_id.as_deref() != Some(*uid));
            mcp::forget_user(d, uid);
            twofactor::forget(d, uid);
            json!({"ok":true,"workspaces_transferred":transferred,"workspaces_deleted":removed})
        }
        // Your overview page's widgets.
        ("GET", ["account", "dashboard"]) => {
            let a = d
                .users
                .iter()
                .find(|a| a.user.id == u.id)
                .ok_or_else(missing)?;
            json!({ "widgets": a.dashboard })
        }
        ("PUT", ["account", "dashboard"]) => {
            let widgets = valid_dashboard(&v["widgets"])?;
            let a = d
                .users
                .iter_mut()
                .find(|a| a.user.id == u.id)
                .ok_or_else(missing)?;
            a.dashboard = widgets;
            json!({ "widgets": a.dashboard })
        }
        // Changes your own password; other sessions end, this one stays.
        ("POST", ["account", "password"]) => {
            attempts.check(&format!("password:{}", u.id))?;
            let a = d
                .users
                .iter_mut()
                .find(|a| a.user.id == u.id)
                .ok_or_else(missing)?;
            let current = v["current_password"].as_str().unwrap_or("");
            let valid = current.len() <= 256
                && PasswordHash::new(&a.password_hash).is_ok_and(|h| {
                    Argon2::default()
                        .verify_password(current.as_bytes(), &h)
                        .is_ok()
                });
            if !valid {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "Your current password is not correct".into(),
                ));
            }
            a.password_hash = password_hash(v["new_password"].as_str().unwrap_or(""))?;
            a.must_change_password = false;
            let created: Option<String> = None;
            let keep = session_token(headers).map(hash_token);
            d.sessions
                .retain(|key, s| s.user_id != u.id || keep.as_ref() == Some(key));
            twofactor::forget(d, &u.id);
            json!({"ok":true,"workspace_id":created})
        }
        ("PUT", ["workspace-access"]) => {
            // Owners share their own workspaces; administrators too, but
            // only the workspaces they own.
            let wid = field(&v, "workspace_id")?;
            let uid = field(&v, "user_id")?;
            // Refused before anything else, so nothing is revealed.
            if !edits_workspace(d, &u, &wid) {
                return Err(forbidden());
            }
            if d.workspaces.iter().any(|w| w.id == wid && w.personal) {
                return Err(bad("A personal workspace cannot be shared"));
            }
            if !d
                .users
                .iter()
                .any(|a| a.user.id == uid && !a.user.admin && !a.user.disabled)
            {
                return Err(missing());
            }
            if d.workspaces
                .iter()
                .any(|w| w.id == wid && w.owner_id == uid)
            {
                return Err(bad("The owner always has edit access"));
            }
            let role = match v["role"].as_str() {
                Some("viewer") => Some(Role::Viewer),
                Some("editor") => Some(Role::Editor),
                Some("none") => None,
                _ => return Err(bad("Invalid role")),
            };
            let had = d.workspace_grants.iter().any(|g| {
                g.workspace_id == wid && g.user_id == uid && Some(&g.role) == role.as_ref()
            });
            if let (false, Some(r)) = (had, &role) {
                let name = d
                    .workspaces
                    .iter()
                    .find(|w| w.id == wid)
                    .map(|w| w.name.clone())
                    .unwrap_or_default();
                mail::access(
                    d,
                    &u,
                    &uid,
                    &format!("the workspace “{name}”"),
                    *r == Role::Editor,
                );
            }
            d.workspace_grants
                .retain(|g| g.workspace_id != wid || g.user_id != uid);
            if let Some(role) = role {
                d.workspace_grants.push(WorkspaceGrant {
                    workspace_id: wid,
                    user_id: uid,
                    role,
                });
            }
            json!({"ok":true})
        }
        ("PUT", ["permissions"]) => {
            // Owners share their own workspaces; administrators too, but
            // only the workspaces they own.
            let fid = field(&v, "folder_id")?;
            let uid = field(&v, "user_id")?;
            let workspace = d
                .folders
                .iter()
                .find(|f| f.id == fid)
                .map(|f| f.workspace_id.clone())
                .unwrap_or_default();
            if !edits_workspace(d, &u, &workspace) {
                return Err(forbidden());
            }
            if d.workspaces.iter().any(|w| w.id == workspace && w.personal) {
                return Err(bad("A personal workspace cannot be shared"));
            }
            if !d
                .users
                .iter()
                .any(|a| a.user.id == uid && !a.user.admin && !a.user.disabled)
            {
                return Err(missing());
            }
            let role = match v["role"].as_str() {
                Some("viewer") => Some(Role::Viewer),
                Some("editor") => Some(Role::Editor),
                Some("none") => None,
                _ => return Err(bad("Invalid role")),
            };
            let had = d
                .grants
                .iter()
                .any(|g| g.folder_id == fid && g.user_id == uid && Some(&g.role) == role.as_ref());
            if let (false, Some(r)) = (had, &role) {
                let folder = d
                    .folders
                    .iter()
                    .find(|f| f.id == fid)
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                let space = d
                    .workspaces
                    .iter()
                    .find(|w| w.id == workspace)
                    .map(|w| w.name.clone())
                    .unwrap_or_default();
                mail::access(
                    d,
                    &u,
                    &uid,
                    &format!("the folder “{folder}” in “{space}”"),
                    *r == Role::Editor,
                );
            }
            d.grants.retain(|g| g.folder_id != fid || g.user_id != uid);
            if let Some(role) = role {
                d.grants.push(Grant {
                    folder_id: fid,
                    user_id: uid,
                    role,
                });
            }
            json!({"ok":true})
        }
        _ => return Err(missing()),
    };
    Ok((result, None))
}
