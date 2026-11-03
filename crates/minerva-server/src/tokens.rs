//! Personal API tokens: for scripts and the `minerva-cli` command, they sign in
//! as their owner through `Authorization: Bearer mnv_api_v1_…`, with the same
//! permissions. They are named, expire (1–365 days) and can be revoked, and they
//! cannot manage the account itself: tokens, MCP credentials, password, second
//! factor, signing out. Server administrator accounts cannot have them.
use super::*;

pub(super) const PREFIX: &str = "mnv_api_v1_";
const LIMIT: usize = 20;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ApiToken {
    id: String,
    user_id: String,
    name: String,
    token_hash: String,
    created_at: u64,
    expires_at: u64,
}
fn view(t: &ApiToken) -> Value {
    json!({"id":t.id,"name":t.name,"created_at":t.created_at,"expires_at":t.expires_at})
}

/// The user a personal token signs in as, if it is current.
pub(super) fn user(d: &Data, token: &str) -> Option<User> {
    let hash = hash_token(token);
    let t = d
        .api_tokens
        .iter()
        .find(|t| t.token_hash == hash && t.expires_at > now())?;
    d.users
        .iter()
        .find(|a| a.user.id == t.user_id && !a.user.disabled && (!a.user.admin || d.local))
        .map(|a| a.user.clone())
}
/// Whether the request signs in with a personal token.
pub(super) fn used(headers: &HeaderMap) -> bool {
    session_token(headers).is_some_and(|t| t.starts_with(PREFIX))
}
/// Paths a personal token may not use: the account's own security.
pub(super) fn reserved(parts: &[&str]) -> bool {
    matches!(
        parts.first(),
        Some(&("account" | "api-tokens" | "mcp-credentials" | "calendar-feed" | "logout"))
    )
}

/// `api-tokens`: list, create, revoke your tokens.
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    match (method, parts) {
        ("GET", [_]) => Ok(json!(d
            .api_tokens
            .iter()
            .filter(|t| t.user_id == u.id)
            .map(view)
            .collect::<Vec<_>>())),
        ("POST", [_]) => {
            if u.admin && !d.local {
                return Err(bad(
                    "Server administrator accounts cannot have API tokens; use a member account",
                ));
            }
            let name = field(v, "name")?.trim().to_string();
            if name.is_empty() || name.chars().count() > 80 {
                return Err(bad("A token has a name of up to 80 characters"));
            }
            let days = v
                .get("expires_days")
                .map(|v| v.as_u64().ok_or_else(|| bad("Expiry must be 1–365 days")))
                .transpose()?
                .unwrap_or(90);
            if !(1..=365).contains(&days) {
                return Err(bad("Expiry must be 1–365 days"));
            }
            d.api_tokens.retain(|t| t.expires_at > now());
            if d.api_tokens.iter().filter(|t| t.user_id == u.id).count() >= LIMIT {
                return Err(bad("Revoke an existing token before creating another"));
            }
            let token = format!("{PREFIX}{}{}", id(), id()).replace('-', "");
            let t = ApiToken {
                id: id(),
                user_id: u.id.clone(),
                name,
                token_hash: hash_token(&token),
                created_at: now(),
                expires_at: now() + days * 86400,
            };
            let mut result = view(&t);
            // Shown once: only its hash is kept.
            result["token"] = json!(token);
            d.api_tokens.push(t);
            Ok(result)
        }
        ("DELETE", [_, tid]) => {
            let before = d.api_tokens.len();
            d.api_tokens
                .retain(|t| !(t.id == *tid && t.user_id == u.id));
            if d.api_tokens.len() == before {
                return Err(missing());
            }
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}
/// Revokes every token of a deleted account.
pub(super) fn forget_user(d: &mut Data, user_id: &str) {
    d.api_tokens.retain(|t| t.user_id != user_id);
}
