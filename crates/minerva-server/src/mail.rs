//! Email: account invitations, password resets, access grants, assignments
//! (`@person@example.com` in a note) and reminders (`⏰2026-10-27 09:00` in a
//! note, scheduled with `/remindme`). Messages wait in an outbox saved with the
//! rest of the data, and a background task sends them over SMTP, retrying with
//! backoff. Without `MINERVA_SMTP_HOST` and `MINERVA_MAIL_FROM`, nothing is
//! queued and reminders are not offered.
use crate::{
    bad, field, forbidden, hash_token, id, missing, note_access, note_role, now, persist, ApiError,
    Data, Note, Shared,
};
use lettre::{
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use minerva_core::User;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::OnceLock, time::Duration};
use tokio::sync::Notify;

/// A message waiting to be sent.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Outgoing {
    id: String,
    to: String,
    subject: String,
    text: String,
    html: String,
    #[serde(default)]
    attempts: u32,
    #[serde(default)]
    next_try: u64,
}
/// A `/remindme` reminder: at `at`, the block's text is mailed to its author.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Reminder {
    id: String,
    user_id: String,
    note_id: String,
    block_id: String,
    at: u64,
}
/// A password reset or first-password link, stored by token hash.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Reset {
    user_id: String,
    expires: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Security {
    Tls,
    StartTls,
    Plain,
}
struct Config {
    host: String,
    port: Option<u16>,
    security: Security,
    credentials: Option<Credentials>,
    from: Mailbox,
}
fn config() -> Option<&'static Config> {
    static CONFIG: OnceLock<Option<Config>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
            let host = var("MINERVA_SMTP_HOST")?;
            let from = match var("MINERVA_MAIL_FROM")?.parse() {
                Ok(from) => from,
                Err(_) => {
                    eprintln!("MINERVA_MAIL_FROM is not a valid address; email is off");
                    return None;
                }
            };
            let security = match var("MINERVA_SMTP_SECURITY").as_deref() {
                Some("tls") => Security::Tls,
                Some("none") => Security::Plain,
                _ => Security::StartTls,
            };
            let credentials = var("MINERVA_SMTP_USERNAME").map(|user| {
                Credentials::new(user, var("MINERVA_SMTP_PASSWORD").unwrap_or_default())
            });
            Some(Config {
                host,
                port: var("MINERVA_SMTP_PORT").and_then(|p| p.parse().ok()),
                security,
                credentials,
                from,
            })
        })
        .as_ref()
}
/// Whether the server can send email.
pub(crate) fn enabled() -> bool {
    config().is_some()
}
/// The address links in emails point to.
fn base_url() -> String {
    std::env::var("MINERVA_PUBLIC_URL")
        .map(|u| u.trim_end_matches('/').to_string())
        .unwrap_or_else(|_| "http://localhost:3000".into())
}
fn wake() -> &'static Notify {
    static WAKE: OnceLock<Notify> = OnceLock::new();
    WAKE.get_or_init(Notify::new)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
/// Queues a message: a few paragraphs, an optional quoted excerpt, and a button.
fn queue(
    d: &mut Data,
    to: &str,
    subject: &str,
    paragraphs: &[String],
    quote: Option<&str>,
    link: (&str, &str),
) -> bool {
    if !enabled() {
        return false;
    }
    let (label, url) = link;
    let mut text = paragraphs.join("\n\n");
    let mut html = String::new();
    for p in paragraphs {
        html += &format!("<p>{}</p>", escape(p));
    }
    if let Some(quote) = quote {
        text += &format!(
            "\n\n{}",
            quote
                .lines()
                .map(|l| format!("> {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        html += &format!(
            "<blockquote style=\"margin:16px 0;padding:8px 14px;border-left:3px solid #5865d8;color:#333;white-space:pre-wrap\">{}</blockquote>",
            escape(quote)
        );
    }
    text += &format!("\n\n{label}: {url}\n");
    html += &format!(
        "<p><a href=\"{}\" style=\"display:inline-block;padding:10px 18px;background:#2a493b;color:#fff;border-radius:8px;text-decoration:none\">{}</a></p><p style=\"color:#888;font-size:12px\">Sent by Minerva · {}</p>",
        escape(url),
        escape(label),
        escape(&base_url())
    );
    let html = format!(
        "<!doctype html><html><body style=\"font-family:system-ui,sans-serif;font-size:15px;line-height:1.5;color:#1d2432;max-width:560px\">{html}</body></html>"
    );
    // A busy server must not build an unbounded backlog.
    if d.mail_outbox.len() >= 1000 {
        return false;
    }
    d.mail_outbox.push(Outgoing {
        id: id(),
        to: to.into(),
        subject: subject.into(),
        text,
        html,
        attempts: 0,
        next_try: 0,
    });
    wake().notify_one();
    true
}

/// A one-time link that lets the account choose a new password.
fn reset_link(d: &mut Data, user_id: &str, valid_for: u64) -> String {
    let token = format!("{}{}", id(), id()).replace('-', "");
    d.password_resets
        .retain(|_, r| r.expires > now() && r.user_id != user_id);
    d.password_resets.insert(
        hash_token(&token),
        Reset {
            user_id: user_id.into(),
            expires: now() + valid_for,
        },
    );
    format!("{}/?reset={token}", base_url())
}
/// Tells a new member about their account, with a link to choose a password.
pub(crate) fn invite(d: &mut Data, by: &User, user: &User) -> bool {
    if !enabled() {
        return false;
    }
    let link = reset_link(d, &user.id, 7 * 86400);
    queue(
        d,
        &user.email,
        "Your Minerva account is ready",
        &[
            format!("Hi {},", user.name),
            format!(
                "{} created a Minerva account for you. You sign in with {}.",
                by.name, user.email
            ),
            "Choose your password with the button below. The link works once, for 7 days; after that, ask for a new one with “Forgot your password?” on the sign-in page.".into(),
        ],
        None,
        ("Choose your password", &link),
    )
}
/// Public endpoints: ask for a reset link, and use it.
pub(crate) fn reset_route(d: &mut Data, path: &str, v: &Value) -> Result<Option<Value>, ApiError> {
    match path {
        "password-reset" => {
            let email = field(v, "email")?.to_lowercase();
            if !enabled() {
                return Err(bad(
                    "This server cannot send email. Ask your administrator to reset your password.",
                ));
            }
            // The same answer either way, so it does not reveal who has an account.
            if let Some(a) = d
                .users
                .iter()
                .find(|a| a.user.email == email && !a.user.disabled)
            {
                let user = a.user.clone();
                let link = reset_link(d, &user.id, 3600);
                queue(
                    d,
                    &user.email,
                    "Reset your Minerva password",
                    &[
                        format!("Hi {},", user.name),
                        "Someone asked to reset the password of your Minerva account. If it was you, choose a new password with the button below; the link works once, for one hour.".into(),
                        "If it was not you, ignore this email: your password stays the same.".into(),
                    ],
                    None,
                    ("Choose a new password", &link),
                );
            }
            Ok(Some(json!({"ok":true})))
        }
        "password-reset/complete" => {
            let key = hash_token(v["token"].as_str().unwrap_or(""));
            let reset = d
                .password_resets
                .get(&key)
                .filter(|r| r.expires > now())
                .cloned()
                .ok_or_else(|| {
                    bad("This link has expired or was already used. Ask for a new one.")
                })?;
            let hash = crate::password_hash(v["password"].as_str().unwrap_or(""))?;
            let a = d
                .users
                .iter_mut()
                .find(|a| a.user.id == reset.user_id && !a.user.disabled)
                .ok_or_else(missing)?;
            a.password_hash = hash;
            a.must_change_password = false;
            let email = a.user.email.clone();
            d.password_resets.retain(|_, r| r.user_id != reset.user_id);
            d.sessions.retain(|_, s| s.user_id != reset.user_id);
            Ok(Some(json!({"ok":true,"email":email})))
        }
        _ => Ok(None),
    }
}

/// Tells someone they were given access to a workspace or a folder.
pub(crate) fn access(d: &mut Data, by: &User, user_id: &str, place: &str, editor: bool) {
    let Some(user) = d
        .users
        .iter()
        .find(|a| a.user.id == user_id)
        .map(|a| a.user.clone())
    else {
        return;
    };
    queue(
        d,
        &user.email,
        &format!("{} shared {place} with you", by.name),
        &[
            format!("Hi {},", user.name),
            format!(
                "{} gave you access to {place}: you can {} it.",
                by.name,
                if editor { "view and edit" } else { "view" }
            ),
        ],
        None,
        ("Open Minerva", &base_url()),
    );
}

/// Email addresses written as `@person@example.com`, outside code.
pub(crate) fn mentions(markdown: &str) -> Vec<String> {
    let mut out = vec![];
    let mut in_fence = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if in_fence {
            continue;
        }
        for (i, _) in line.match_indices('@') {
            if line[..i]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace() && c != '(')
            {
                continue;
            }
            let len = mention_len(&line[i + 1..]);
            if len > 0 {
                let email = line[i + 1..i + 1 + len].to_lowercase();
                if !out.contains(&email) {
                    out.push(email);
                }
            }
        }
    }
    out
}
/// The length of the email address that starts `text`, or 0.
pub(crate) fn mention_len(text: &str) -> usize {
    let end = text
        .find(|c: char| {
            !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '@'))
        })
        .unwrap_or(text.len());
    let candidate = text[..end].trim_end_matches(['.', '-']);
    let Some((local, domain)) = candidate.split_once('@') else {
        return 0;
    };
    let valid = !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && domain.split('.').all(|p| !p.is_empty())
        && domain
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
    if valid {
        candidate.len()
    } else {
        0
    }
}
fn note_link(note: &Note) -> String {
    format!("{}/?note={}", base_url(), note.id)
}
/// Emails people newly assigned in a note (`@person@example.com` added to a
/// block) who can open it. `actor` is who made the change.
pub(crate) fn assignments(old: &Data, d: &mut Data, actor: Option<&User>) {
    if !enabled() {
        return;
    }
    let mut out: Vec<(User, Note, String)> = vec![];
    for n in d.notes.iter().filter(|n| !n.trashed) {
        let before = old.notes.iter().find(|o| o.id == n.id);
        for b in &n.blocks {
            let had = before
                .and_then(|o| o.blocks.iter().find(|x| x.id == b.id))
                .map(|x| mentions(&x.markdown))
                .unwrap_or_default();
            for email in mentions(&b.markdown)
                .into_iter()
                .filter(|e| !had.contains(e))
            {
                let Some(user) = d
                    .users
                    .iter()
                    .find(|a| a.user.email == email && !a.user.disabled)
                    .map(|a| a.user.clone())
                else {
                    continue;
                };
                if actor.is_some_and(|a| a.id == user.id) || note_role(d, &user, n).is_none() {
                    continue;
                }
                if out.len() < 20 {
                    out.push((user, n.clone(), b.markdown.clone()));
                }
            }
        }
    }
    let by = actor.map_or("Someone".to_string(), |a| a.name.clone());
    for (user, note, block) in out {
        queue(
            d,
            &user.email,
            &format!("{by} assigned you in “{}”", note.title),
            &[
                format!("Hi {},", user.name),
                format!("{by} assigned you in “{}”:", note.title),
            ],
            Some(&block),
            ("Open the note", &note_link(&note)),
        );
    }
}

/// `GET notes/{id}/people`: the members who can open a note, for the `@`
/// picker; `GET mentions`: notes that mention you, newest first.
pub(crate) fn people_route(d: &Data, u: &User, parts: &[&str]) -> Result<Option<Value>, ApiError> {
    match parts {
        ["notes", nid, "people"] => {
            let n = d.notes.iter().find(|n| n.id == *nid).ok_or_else(missing)?;
            note_access(d, u, n, false)?;
            let mut people: Vec<Value> = d
                .users
                .iter()
                .filter(|a| !a.user.disabled && !a.user.admin)
                .filter(|a| note_role(d, &a.user, n).is_some())
                .map(|a| json!({"name": a.user.name, "email": a.user.email, "me": a.user.id == u.id}))
                .collect();
            people.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_lowercase());
            Ok(Some(json!(people)))
        }
        ["mentions"] => {
            let email = u.email.to_lowercase();
            let mut found: Vec<(u64, Value)> = d
                .notes
                .iter()
                .filter(|n| !n.trashed && note_role(d, u, n).is_some())
                .filter_map(|n| {
                    let block = n.blocks.iter().find(|b| mentions(&b.markdown).contains(&email))?;
                    let excerpt: String = block.markdown.chars().take(160).collect();
                    Some((n.updated_at, json!({"id": n.id, "title": n.title, "icon": n.icon, "updated_at": n.updated_at, "excerpt": excerpt})))
                })
                .collect();
            found.sort_by(|a, b| b.0.cmp(&a.0));
            Ok(Some(json!(found
                .into_iter()
                .take(20)
                .map(|(_, v)| v)
                .collect::<Vec<_>>())))
        }
        _ => Ok(None),
    }
}
/// `POST reminders {note_id, block_id, at}` schedules one; `GET reminders` lists
/// yours; `DELETE reminders/{id}` cancels one.
pub(crate) fn reminders_route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    match (method, parts) {
        ("GET", ["reminders"]) => Ok(json!({
            "enabled": enabled(),
            "reminders": d.reminders.iter().filter(|r| r.user_id == u.id).map(|r| json!({"id":r.id,"note_id":r.note_id,"block_id":r.block_id,"at":r.at})).collect::<Vec<_>>(),
        })),
        ("POST", ["reminders"]) => {
            if !enabled() {
                return Err(bad(
                    "This server cannot send email, so it cannot send reminders",
                ));
            }
            let note_id = field(v, "note_id")?;
            let block_id = field(v, "block_id")?;
            let at = v["at"]
                .as_u64()
                .ok_or_else(|| bad("Choose when to be reminded"))?;
            if at < now().saturating_sub(60) || at > now() + 5 * 365 * 86400 {
                return Err(bad("Choose a time in the future"));
            }
            let n = d
                .notes
                .iter()
                .find(|n| n.id == note_id)
                .ok_or_else(missing)?;
            note_access(d, u, n, false)?;
            if d.reminders.iter().filter(|r| r.user_id == u.id).count() >= 500 {
                return Err(bad("You have too many reminders scheduled"));
            }
            let r = Reminder {
                id: id(),
                user_id: u.id.clone(),
                note_id,
                block_id,
                at,
            };
            d.reminders.push(r.clone());
            wake().notify_one();
            Ok(json!({"id": r.id, "at": r.at}))
        }
        ("DELETE", ["reminders", rid]) => {
            let before = d.reminders.len();
            d.reminders.retain(|r| r.id != *rid || r.user_id != u.id);
            if before == d.reminders.len() {
                return Err(forbidden());
            }
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}
/// Queues the reminders that are due. A reminder whose block is gone, or no
/// longer holds a `⏰` time, was removed by the person, so it is dropped.
fn fire_reminders(d: &mut Data) -> bool {
    let due: Vec<Reminder> = d
        .reminders
        .iter()
        .filter(|r| r.at <= now())
        .cloned()
        .collect();
    if due.is_empty() {
        return false;
    }
    d.reminders.retain(|r| r.at > now());
    for r in due {
        let Some(user) = d
            .users
            .iter()
            .find(|a| a.user.id == r.user_id && !a.user.disabled)
            .map(|a| a.user.clone())
        else {
            continue;
        };
        let Some(note) = d
            .notes
            .iter()
            .find(|n| n.id == r.note_id && !n.trashed)
            .cloned()
        else {
            continue;
        };
        let Some(block) = note
            .blocks
            .iter()
            .find(|b| b.id == r.block_id && b.markdown.contains('⏰'))
        else {
            continue;
        };
        if note_role(d, &user, &note).is_none() {
            continue;
        }
        let text = block.markdown.clone();
        queue(
            d,
            &user.email,
            &format!("Reminder: {}", note.title),
            &[
                format!("Hi {},", user.name),
                format!("You asked to be reminded of this, in “{}”:", note.title),
            ],
            Some(&text),
            ("Open the note", &note_link(&note)),
        );
    }
    true
}

/// Starts the sender, when email is configured.
pub(crate) fn spawn(store: Shared) {
    if !enabled() || tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        loop {
            tick(&store).await;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                _ = wake().notified() => tokio::time::sleep(Duration::from_millis(300)).await,
            }
        }
    });
}
async fn tick(store: &Shared) {
    let pending: Vec<Outgoing> = {
        let Ok(mut s) = store.lock() else { return };
        let mut d = s.data.clone();
        if fire_reminders(&mut d) && persist(&mut s, d).is_err() {
            eprintln!("Could not save due reminders");
        }
        s.data
            .mail_outbox
            .iter()
            .filter(|m| m.next_try <= now())
            .take(50)
            .cloned()
            .collect()
    };
    if pending.is_empty() {
        return;
    }
    let Some(c) = config() else { return };
    let transport = match transport(c) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Email: {e}");
            return;
        }
    };
    let mut results = vec![];
    for m in pending {
        let sent = match message(c, &m) {
            Ok(msg) => transport
                .send(msg)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        if let Err(e) = &sent {
            eprintln!("Email to {} not sent: {e}", m.to);
        }
        results.push((m.id, sent.is_ok()));
    }
    let Ok(mut s) = store.lock() else { return };
    let mut d = s.data.clone();
    for (mid, ok) in results {
        if ok {
            d.mail_outbox.retain(|m| m.id != mid);
        } else if let Some(m) = d.mail_outbox.iter_mut().find(|m| m.id == mid) {
            m.attempts += 1;
            m.next_try = now() + 60 * 2u64.pow(m.attempts.min(8));
        }
    }
    // Gives up after about a day of failures.
    d.mail_outbox.retain(|m| m.attempts < 10);
    if persist(&mut s, d).is_err() {
        eprintln!("Could not save the email outbox");
    }
}
fn transport(c: &Config) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let mut builder = match c.security {
        Security::Tls => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&c.host).map_err(|e| e.to_string())?
        }
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&c.host)
            .map_err(|e| e.to_string())?,
        Security::Plain => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&c.host),
    };
    if let Some(port) = c.port {
        builder = builder.port(port);
    }
    if let Some(credentials) = &c.credentials {
        builder = builder.credentials(credentials.clone());
    }
    Ok(builder.timeout(Some(Duration::from_secs(20))).build())
}
fn message(c: &Config, m: &Outgoing) -> Result<Message, String> {
    Message::builder()
        .from(c.from.clone())
        .to(m
            .to
            .parse()
            .map_err(|_| format!("invalid address {}", m.to))?)
        .subject(&m.subject)
        .multipart(MultiPart::alternative_plain_html(
            m.text.clone(),
            m.html.clone(),
        ))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_mentions() {
        assert_eq!(
            mentions("Ask @ana@test.it and @Bob@x.org."),
            ["ana@test.it", "bob@x.org"]
        );
        assert!(mentions("mail ana@test.it, not a mention").is_empty());
        assert!(mentions("```\n@ana@test.it\n```").is_empty());
        assert!(mentions("@nobody").is_empty());
        assert_eq!(mention_len("ana@test.it). Done"), "ana@test.it".len());
    }
}
