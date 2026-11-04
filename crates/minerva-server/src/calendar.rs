//! A calendar feed: someone's reminders, and optionally their journal days, as
//! an iCalendar (`.ics`) address that calendar apps subscribe to.
//!
//! Calendar apps cannot sign in, so the address holds a secret
//! (`/calendar/mnv_cal_….ics`). Only its hash is kept; making a new address
//! replaces the old one, and turning the feed off removes it. The feed shows
//! what its owner could see in the app at the time it is fetched.
//!
//! - Workspace reminders: at their time, with their repeat as a rule and an
//!   alert.
//! - `⏰` times written in notes the owner can open (which include their
//!   `/remindme` reminders): at that local time, as written ("floating" times,
//!   with no time zone).
//! - Journal days, when chosen: all-day events linking to the entry.
//!
//! Besides one's own feed (every workspace), a feed can hold one workspace
//! only, as an address to share with people outside it.
//!
//! A repeating reminder set in a known time zone is written at its local time
//! in that zone (`TZID`), so calendar apps keep it there across
//! daylight-saving changes; other times are in UTC.
use super::*;
use axum::extract::Path;

const PREFIX: &str = "mnv_cal_";
/// The most events one feed holds.
const LIMIT: usize = 2000;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Feed {
    user_id: String,
    token_hash: String,
    created_at: u64,
    #[serde(default)]
    journal: bool,
    /// One workspace only; none for all of the owner's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_id: Option<String>,
}

fn view(f: Option<&Feed>) -> Value {
    match f {
        Some(f) => {
            json!({"enabled": true, "created_at": f.created_at, "journal": f.journal, "workspace_id": f.workspace_id})
        }
        None => json!({"enabled": false, "journal": false}),
    }
}

/// `calendar-feed` (all your workspaces) and `calendar-feed/{workspace}` (one,
/// to share): `GET` whether it is on; `POST {journal?}` makes a new address
/// (shown once; the old one stops working); `PUT {journal}` changes what it
/// holds; `DELETE` turns it off.
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    workspace: Option<&str>,
    v: &Value,
    headers: &HeaderMap,
) -> Result<Value, ApiError> {
    if let Some(w) = workspace {
        if !reminders::sees_workspace(d, u, w) {
            return Err(forbidden());
        }
    }
    let mine = |f: &Feed| f.user_id == u.id && f.workspace_id.as_deref() == workspace;
    match method {
        "GET" => Ok(view(d.calendar_feeds.iter().find(|f| mine(f)))),
        "POST" => {
            let journal = v["journal"].as_bool().unwrap_or(false);
            d.calendar_feeds.retain(|f| !mine(f));
            let token = format!("{PREFIX}{}{}", id(), id()).replace('-', "");
            let feed = Feed {
                user_id: u.id.clone(),
                token_hash: hash_token(&token),
                created_at: now(),
                journal,
                workspace_id: workspace.map(String::from),
            };
            let mut result = view(Some(&feed));
            d.calendar_feeds.push(feed);
            // Shown once: only its hash is kept.
            result["url"] = json!(format!(
                "{}/calendar/{token}.ics",
                sharing::site_url(headers)
            ));
            Ok(result)
        }
        "PUT" => {
            let feed = d
                .calendar_feeds
                .iter_mut()
                .find(|f| mine(f))
                .ok_or_else(missing)?;
            feed.journal = v["journal"].as_bool().unwrap_or(feed.journal);
            let feed = feed.clone();
            Ok(view(Some(&feed)))
        }
        "DELETE" => {
            d.calendar_feeds.retain(|f| !mine(f));
            Ok(view(None))
        }
        _ => Err(missing()),
    }
}

/// Turns off the feed of a deleted account.
pub(super) fn forget_user(d: &mut Data, user_id: &str) {
    d.calendar_feeds.retain(|f| f.user_id != user_id);
}

/// `GET /calendar/{token}.ics`.
pub(super) async fn feed(
    State(store): State<Shared>,
    Path(file): Path<String>,
    headers: HeaderMap,
) -> Response {
    let not_found = || (StatusCode::NOT_FOUND, "Not found").into_response();
    let Some(token) = file.strip_suffix(".ics").filter(|t| t.starts_with(PREFIX)) else {
        return not_found();
    };
    let Ok(s) = store.lock() else {
        return not_found();
    };
    let d = &s.data;
    let hash = hash_token(token);
    let Some(feed) = d.calendar_feeds.iter().find(|f| f.token_hash == hash) else {
        return not_found();
    };
    let Some(u) = d
        .users
        .iter()
        .find(|a| a.user.id == feed.user_id && !a.user.disabled)
        .map(|a| a.user.clone())
    else {
        return not_found();
    };
    let body = calendar(
        d,
        &u,
        feed.journal,
        feed.workspace_id.as_deref(),
        &sharing::site_url(&headers),
        now(),
    );
    (
        [
            ("content-type", "text/calendar; charset=utf-8"),
            ("content-disposition", "inline; filename=\"minerva.ics\""),
            ("referrer-policy", "no-referrer"),
            ("x-robots-tag", "noindex"),
        ],
        body,
    )
        .into_response()
}

/// The feed's iCalendar text for `u`.
pub(super) fn calendar(
    d: &Data,
    u: &User,
    journal: bool,
    only: Option<&str>,
    base: &str,
    at: u64,
) -> String {
    // A workspace feed holds that workspace only.
    let in_scope = |w: &str| only.is_none_or(|o| o == w);
    let stamp = utc(at);
    let mut events: Vec<Vec<String>> = vec![];
    let workspace_of = |folder: &str| {
        let w = d
            .folders
            .iter()
            .find(|f| f.id == folder)
            .map_or("", |f| f.workspace_id.as_str());
        d.workspaces
            .iter()
            .find(|x| x.id == w)
            .map_or(String::new(), |x| x.name.clone())
    };
    for r in reminders::listed(d, u).filter(|r| r.note_id.is_empty() && in_scope(&r.workspace_id)) {
        let workspace = d
            .workspaces
            .iter()
            .find(|w| w.id == r.workspace_id)
            .map_or("", |w| w.name.as_str());
        let mut event = vec![
            format!("UID:reminder-{}@minerva", r.id),
            format!("DTSTAMP:{stamp}"),
            // A repeat in a known time zone keeps its local time there.
            match (&r.repeat, r.zone.is_empty()) {
                (Some(_), false) => format!(
                    "DTSTART;TZID={}:{}",
                    r.zone,
                    utc(r.at.saturating_add_signed(i64::from(r.offset) * 60)).trim_end_matches('Z')
                ),
                _ => format!("DTSTART:{}", utc(r.at)),
            },
            "DURATION:PT30M".into(),
            format!("SUMMARY:{}", text(&r.title)),
            format!("DESCRIPTION:{}", text(&format!("Reminder in {workspace}"))),
            format!("URL:{base}/?view=reminders"),
        ];
        if !r.category.is_empty() {
            event.push(format!("CATEGORIES:{}", text(&r.category)));
        }
        if let Some(repeat) = r.repeat {
            event.push(format!("RRULE:{}", repeat.rrule()));
        }
        event.extend(alarm(&r.title));
        events.push(event);
    }
    for n in d
        .notes
        .iter()
        .filter(|n| !n.trashed && note_role(d, u, n).is_some())
        .filter(|n| only.is_none() || in_scope(&workspace_of_folder(d, &n.folder_id)))
    {
        if journal {
            if let Some(day) = n.journal.as_deref().and_then(compact_date) {
                let workspace = workspace_of(&n.folder_id);
                let summary = if n.title == n.journal.clone().unwrap_or_default() {
                    format!("Journal · {workspace}")
                } else {
                    n.title.clone()
                };
                events.push(vec![
                    format!("UID:journal-{}@minerva", n.id),
                    format!("DTSTAMP:{stamp}"),
                    format!("DTSTART;VALUE=DATE:{day}"),
                    format!("SUMMARY:{}", text(&summary)),
                    format!("URL:{base}/?note={}", n.id),
                    "TRANSP:TRANSPARENT".into(),
                ]);
            }
        }
        for b in n.blocks.iter().filter(|b| b.markdown.contains('⏰')) {
            for (i, when) in reminders::stamps(&b.markdown).iter().enumerate() {
                let Some(start) = floating(when) else {
                    continue;
                };
                let summary = block_summary(&b.markdown).unwrap_or_else(|| n.title.clone());
                events.push(
                    [
                        vec![
                            format!("UID:note-{}-{}-{i}@minerva", n.id, b.id),
                            format!("DTSTAMP:{stamp}"),
                            format!("DTSTART:{start}"),
                            "DURATION:PT30M".into(),
                            format!("SUMMARY:{}", text(&summary)),
                            format!("DESCRIPTION:{}", text(&format!("In “{}”", n.title))),
                            format!("URL:{base}/?note={}&block={}", n.id, b.id),
                        ],
                        alarm(&summary),
                    ]
                    .concat(),
                );
            }
        }
        if events.len() >= LIMIT {
            break;
        }
    }
    let name = format!("Minerva · {}", u.name);
    let mut out = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".into(),
        "PRODID:-//Minerva//Calendar feed//EN".into(),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
        format!("X-WR-CALNAME:{}", text(&name)),
        format!("NAME:{}", text(&name)),
        "REFRESH-INTERVAL;VALUE=DURATION:PT1H".into(),
        "X-PUBLISHED-TTL:PT1H".into(),
    ];
    for event in events.into_iter().take(LIMIT) {
        out.push("BEGIN:VEVENT".into());
        out.extend(event);
        out.push("END:VEVENT".into());
    }
    out.push("END:VCALENDAR".into());
    out.iter()
        .map(|line| fold(line))
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}

/// An alert when the event starts.
fn workspace_of_folder(d: &Data, folder: &str) -> String {
    d.folders
        .iter()
        .find(|f| f.id == folder)
        .map_or(String::new(), |f| f.workspace_id.clone())
}
fn alarm(summary: &str) -> Vec<String> {
    vec![
        "BEGIN:VALARM".into(),
        "ACTION:DISPLAY".into(),
        "TRIGGER:PT0M".into(),
        format!("DESCRIPTION:{}", text(summary)),
        "END:VALARM".into(),
    ]
}

/// A block's words for an event's name: its first line without Markdown or
/// the `⏰` time, up to 120 characters.
fn block_summary(markdown: &str) -> Option<String> {
    let line = markdown.lines().find(|l| !l.trim().is_empty())?;
    let mut text = line.to_string();
    for stamp in reminders::stamps(line) {
        text = text.replace(&format!("⏰{stamp}"), "");
        text = text.replace(&format!("⏰{}", stamp.replace(' ', "T")), "");
    }
    let text = text
        .trim_start()
        .trim_start_matches(['#', '>', '-', '*', '+'])
        .trim_start()
        .trim_start_matches("[ ]")
        .trim_start_matches("[x]")
        .trim_start_matches("[X]")
        .replace(['*', '_', '`', '~', '='], "");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| text.chars().take(120).collect())
}

/// Unix seconds as an iCalendar UTC time (`20261027T080000Z`).
fn utc(at: u64) -> String {
    let (year, month, day) = reminders::civil((at / 86_400) as i64);
    let s = at % 86_400;
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}
/// `2026-10-27 09:00` as a floating local time (`20261027T090000`).
fn floating(stamp: &str) -> Option<String> {
    let date = compact_date(stamp.get(..10)?)?;
    let (hour, minute) = (stamp.get(11..13)?, stamp.get(14..16)?);
    let valid = hour.parse::<u32>().ok()? < 24 && minute.parse::<u32>().ok()? < 60;
    valid.then(|| format!("{date}T{hour}{minute}00"))
}
/// `2026-10-27` as `20261027`, for real dates only.
fn compact_date(date: &str) -> Option<String> {
    let mut parts = date.splitn(3, '-').map(|p| p.parse::<u32>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    let valid = date.len() == 10 && (1..=12).contains(&month) && (1..=31).contains(&day);
    valid.then(|| format!("{year:04}{month:02}{day:02}"))
}
/// Text escaped for an iCalendar value.
fn text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace(['\n', '\r'], "\\n")
}
/// A content line folded at 75 bytes, without splitting a character.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / 70 * 3);
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_text_and_folding() {
        assert_eq!(utc(0), "19700101T000000Z");
        assert_eq!(utc(1_793_091_600), "20261027T090000Z");
        assert_eq!(
            floating("2026-10-27 09:05").as_deref(),
            Some("20261027T090500")
        );
        assert_eq!(floating("2026-13-27 09:05"), None);
        assert_eq!(floating("2026-10-27 25:00"), None);
        assert_eq!(text("a, b; c\\d\ne"), "a\\, b\\; c\\\\d\\ne");
        let long = format!("SUMMARY:{}", "é".repeat(60));
        let folded = fold(&long);
        assert!(folded.split("\r\n").all(|l| l.len() <= 75));
        assert_eq!(folded.replace("\r\n ", ""), long);
        assert_eq!(
            block_summary("- [ ] **Call** the bank ⏰2026-10-27 09:00").as_deref(),
            Some("Call the bank")
        );
        assert_eq!(block_summary("⏰2026-10-27 09:00"), None);
    }
}
