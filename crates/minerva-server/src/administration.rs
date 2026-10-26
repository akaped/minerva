//! Management-only server accounts and workspace member administration.
use super::*;
use axum::{extract::Request, middleware::Next};

pub(super) async fn restrict(
    State(store): State<Shared>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let blocked = {
        let s = match store.lock() {
            Ok(s) => s,
            Err(_) => return internal().into_response(),
        };
        !s.data.local
            && authenticate(&s.data, request.headers()).is_ok_and(|u| u.admin)
            && path.starts_with("/api/")
            && ![
                "/api/v1/health",
                "/api/v1/login",
                "/api/v1/logout",
                "/api/v1/bootstrap",
                "/api/v1/password-reset",
            ]
            .contains(&path)
            && ![
                "/api/v1/admin",
                "/api/v1/users",
                "/api/v1/account",
                "/api/v1/ai/providers",
            ]
            .iter()
            .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
    };
    if blocked {
        return ApiError(StatusCode::FORBIDDEN,"Server administrators manage accounts and settings. Sign in with a member account to use workspaces.".into()).into_response();
    }
    next.run(request).await
}
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Option<Value>, ApiError> {
    match (method, parts) {
        ("GET", ["workspaces", wid, "members"]) => {
            let w = d
                .workspaces
                .iter()
                .find(|w| w.id == *wid)
                .ok_or_else(missing)?;
            let member = owns(d, u, wid)
                || d.workspace_grants
                    .iter()
                    .any(|g| g.workspace_id == *wid && g.user_id == u.id);
            if !member {
                return Err(forbidden());
            }
            let members:Vec<_>=d.users.iter().filter(|a|!a.user.admin && !a.user.disabled).filter_map(|a|{
                let owner=a.user.id==w.owner_id;
                let role=if owner {Some(Role::Editor)}else{d.workspace_grants.iter().find(|g|g.workspace_id==*wid && g.user_id==a.user.id).map(|g|g.role.clone())};
                if role.is_none() && !edits_workspace(d,u,wid){return None;}
                Some(json!({"id":a.user.id,"name":a.user.name,"email":a.user.email,"role":role,"owner":owner}))
            }).collect();
            Ok(Some(
                json!({"members":members,"can_edit":edits_workspace(d,u,wid),"personal":w.personal}),
            ))
        }
        ("POST", ["admin", "workspaces", wid, "transfer"]) => {
            admin(u)?;
            let target = field(v, "user_id")?;
            if !d
                .users
                .iter()
                .any(|a| a.user.id == target && !a.user.admin && !a.user.disabled)
            {
                return Err(bad("Choose an active member account"));
            }
            let admin_ids: HashSet<_> = d
                .users
                .iter()
                .filter(|a| a.user.admin)
                .map(|a| a.user.id.clone())
                .collect();
            let w = d
                .workspaces
                .iter_mut()
                .find(|w| w.id == *wid)
                .ok_or_else(missing)?;
            if !admin_ids.contains(&w.owner_id) {
                return Err(forbidden());
            }
            w.owner_id = target.clone();
            w.personal = false;
            if w.name == PERSONAL {
                w.name = "Recovered workspace".into();
            }
            d.workspace_grants.retain(|g| {
                g.workspace_id != *wid || (g.user_id != target && !admin_ids.contains(&g.user_id))
            });
            Ok(Some(json!({"ok":true})))
        }
        _ => Ok(None),
    }
}
pub(super) async fn overview(
    State(store): State<Shared>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let s = store.lock().map_err(|_| internal())?;
    let u = authenticate(&s.data, &headers)?;
    admin(&u)?;
    let d = &s.data;
    let mut file_sizes = HashMap::new();
    let mut stmt =
        s.db.prepare("SELECT note_id,SUM(length(content)) FROM attachments GROUP BY note_id")
            .map_err(|_| internal())?;
    for row in stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
        .map_err(|_| internal())?
    {
        let (id, size) = row.map_err(|_| internal())?;
        file_sizes.insert(id, size);
    }
    let users:Vec<_>=d.users.iter().map(|a|{
        let workspaces:HashSet<_>=d.workspaces.iter().filter(|w|w.owner_id==a.user.id).map(|w|w.id.as_str()).collect();
        let folders:HashSet<_>=d.folders.iter().filter(|f|workspaces.contains(f.workspace_id.as_str())).map(|f|f.id.as_str()).collect();
        let notes:Vec<_>=d.notes.iter().filter(|n|folders.contains(n.folder_id.as_str())).collect();
        let files:u64=notes.iter().map(|n|file_sizes.get(&n.id).copied().unwrap_or(0)).sum();
        let text:usize=notes.iter().flat_map(|n|&n.blocks).map(|b|b.markdown.len()).sum();
        json!({"id":a.user.id,"name":a.user.name,"admin":a.user.admin,"workspaces":workspaces.len(),"notes":notes.len(),"text_bytes":text,"file_bytes":files})
    }).collect();
    let legacy: Vec<_> = d
        .workspaces
        .iter()
        .filter(|w| {
            d.users
                .iter()
                .any(|a| a.user.id == w.owner_id && a.user.admin)
        })
        .map(|w| json!({"id":w.id,"name":w.name}))
        .collect();
    Ok(axum::Json(json!({"users":users,"total_users":d.users.len(),"total_workspaces":d.workspaces.len(),"total_notes":d.notes.len(),"file_bytes":file_sizes.values().sum::<u64>(),"legacy_workspaces":legacy})).into_response())
}
