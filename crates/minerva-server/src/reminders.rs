//! Reminders. Two kinds:
//!
//! - **Note reminders** (`/remindme`): a `⏰2026-10-27 09:00` time in a block,
//!   whose block is emailed to the person who set it when it is due, then
//!   forgotten. Personal.
//! - **Workspace reminders**, made on the Reminders page: a title, a category,
//!   and either one time or a repeat ("every 2 years"). They belong to the
//!   workspace: everyone who can open it sees them and is emailed when one is
//!   due, and everyone who can edit in it can change them. A due reminder stays
//!   due until it is marked done; a repeating one then moves to its next time.
//!
//! The Reminders page also lists every `⏰` time written in the notes the person
//! can open, whoever wrote it.
use crate::{
    bad, edits_workspace, field, forbidden, id, mail, missing, note_access, note_role, now, role,
    ApiError, Data, Role,
};
use minerva_core::User;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Unit {
    Day,
    Week,
    Month,
    Year,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub(crate) struct Repeat {
    every: u32,
    unit: Unit,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Reminder {
    pub(crate) id: String,
    pub(crate) user_id: String,
    /// Set for a `/remindme` reminder, empty for one made on the Reminders page.
    #[serde(default)]
    pub(crate) note_id: String,
    #[serde(default)]
    pub(crate) block_id: String,
    /// When it is due next, in Unix seconds.
    pub(crate) at: u64,
    #[serde(default)]
    pub(crate) workspace_id: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) category: String,
    #[serde(default)]
    pub(crate) repeat: Option<Repeat>,
    /// Minutes east of UTC where it was set, so a repeat keeps the local time
    /// of day and the day of the month.
    #[serde(default)]
    pub(crate) offset: i32,
    /// The email for the current time went out; it waits to be marked done.
    #[serde(default)]
    pub(crate) sent: bool,
}

/// Reminders one person can set, and reminders one workspace can hold.
const LIMIT: usize = 500;
const WORKSPACE_LIMIT: usize = 1000;

/// True when `u` can open at least one folder of workspace `w`.
fn sees_workspace(d: &Data, u: &User, w: &str) -> bool {
    d.folders
        .iter()
        .any(|f| f.workspace_id == w && role(d, u, &f.id).is_some())
}

/// True when `u` can edit in workspace `w`: the whole workspace, or a folder of it.
fn writes_workspace(d: &Data, u: &User, w: &str) -> bool {
    edits_workspace(d, u, w)
        || d.folders
            .iter()
            .any(|f| f.workspace_id == w && role(d, u, &f.id) == Some(Role::Editor))
}
/// Note reminders are their owner's; workspace reminders, everyone's who edits there.
fn may_change(d: &Data, u: &User, r: &Reminder) -> bool {
    if r.note_id.is_empty() {
        writes_workspace(d, u, &r.workspace_id)
    } else {
        r.user_id == u.id
    }
}

/// Days since 1970-01-01 to (year, month 1–12, day), and back (Howard Hinnant's
/// civil calendar algorithms).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn month_length(year: i64, month: u32) -> u32 {
    let next = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    (next - days_from_civil(year, month, 1)) as u32
}

/// The time one repeat after `at`. Months and years keep the day of the month,
/// or use the month's last day when it is shorter (31 January → 28 February).
pub(crate) fn next(at: u64, repeat: Repeat, offset: i32) -> u64 {
    let every = i64::from(repeat.every.max(1));
    let local = at as i64 + i64::from(offset) * 60;
    let (days, time) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    let moved = match repeat.unit {
        Unit::Day => days + every,
        Unit::Week => days + 7 * every,
        Unit::Month | Unit::Year => {
            let (year, month, day) = civil(days);
            let months = if repeat.unit == Unit::Year {
                12 * every
            } else {
                every
            };
            let index = year * 12 + i64::from(month) - 1 + months;
            let (year, month) = (index.div_euclid(12), index.rem_euclid(12) as u32 + 1);
            days_from_civil(year, month, day.min(month_length(year, month)))
        }
    };
    (moved * 86_400 + time - i64::from(offset) * 60).max(0) as u64
}

/// `⏰2026-10-27 09:00` times written in a block, as written.
fn stamps(markdown: &str) -> Vec<String> {
    markdown
        .match_indices('⏰')
        .filter_map(|(i, c)| {
            let stamp = markdown[i + c.len()..].get(..16)?;
            let shape = stamp.bytes().enumerate().all(|(j, b)| match j {
                4 | 7 => b == b'-',
                10 => b == b' ' || b == b'T',
                13 => b == b':',
                _ => b.is_ascii_digit(),
            });
            shape.then(|| stamp.replace('T', " "))
        })
        .collect()
}

fn view(d: &Data, u: &User, r: &Reminder) -> Value {
    let by = d
        .users
        .iter()
        .find(|a| a.user.id == r.user_id)
        .map_or("", |a| a.user.name.as_str());
    json!({
        "id": r.id, "note_id": r.note_id, "block_id": r.block_id, "at": r.at,
        "workspace_id": r.workspace_id, "title": r.title, "category": r.category,
        "repeat": r.repeat, "offset": r.offset, "sent": r.sent,
        "created_by": by, "can_edit": may_change(d, u, r),
    })
}

/// Title, category, time, repeat and offset of a reminder made on the page.
fn details(v: &Value) -> Result<(String, String, u64, Option<Repeat>, i32), ApiError> {
    let title = v["title"].as_str().unwrap_or("").trim().to_string();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(bad("Give the reminder a name of 1–200 characters"));
    }
    let category = v["category"].as_str().unwrap_or("").trim().to_string();
    if category.chars().count() > 40 {
        return Err(bad("Categories can be up to 40 characters"));
    }
    let at = v["at"]
        .as_u64()
        .ok_or_else(|| bad("Choose when to be reminded"))?;
    if at > now() + 100 * 366 * 86_400 {
        return Err(bad("Choose a date within the next 100 years"));
    }
    let repeat = match &v["repeat"] {
        Value::Null => None,
        r => {
            let r: Repeat = serde_json::from_value(r.clone())
                .map_err(|_| bad("Choose how often it repeats"))?;
            if !(1..=1000).contains(&r.every) {
                return Err(bad("Repeat every 1 to 1000 days, weeks, months or years"));
            }
            Some(r)
        }
    };
    let offset = v["offset"].as_i64().unwrap_or(0);
    if !(-900..=900).contains(&offset) {
        return Err(bad("Invalid time zone"));
    }
    Ok((title, category, at, repeat, offset as i32))
}

/// - `GET reminders`: your note reminders, the reminders of every workspace you
///   can open, and the `⏰` times in notes you can open.
/// - `POST reminders`: `{note_id, block_id, at}` emails a block (`/remindme`);
///   `{workspace_id, title, category?, at, repeat?, offset?}` makes a reminder
///   in a workspace you can edit in. `repeat` is `{every, unit: day|week|month|year}`.
/// - `PUT reminders/{id}`: changes a workspace reminder.
/// - `POST reminders/{id}/done`: a repeating reminder moves to its next time;
///   any other is removed.
/// - `DELETE reminders/{id}`.
pub(crate) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Value, ApiError> {
    match (method, parts) {
        ("GET", ["reminders"]) => {
            let mine: Vec<Value> = d
                .reminders
                .iter()
                .filter(|r| {
                    if r.note_id.is_empty() {
                        sees_workspace(d, u, &r.workspace_id)
                    } else {
                        r.user_id == u.id
                    }
                })
                .map(|r| view(d, u, r))
                .collect();
            let mut from_notes = vec![];
            for n in d.notes.iter().filter(|n| !n.trashed) {
                if !n.blocks.iter().any(|b| b.markdown.contains('⏰'))
                    || note_role(d, u, n).is_none()
                {
                    continue;
                }
                let workspace = d
                    .folders
                    .iter()
                    .find(|f| f.id == n.folder_id)
                    .map_or("", |f| f.workspace_id.as_str());
                for b in &n.blocks {
                    for stamp in stamps(&b.markdown) {
                        let emailed = d
                            .reminders
                            .iter()
                            .any(|r| r.user_id == u.id && r.note_id == n.id && r.block_id == b.id);
                        let text: String = b.markdown.chars().take(200).collect();
                        if from_notes.len() < 1000 {
                            from_notes.push(json!({
                                "note_id": n.id, "block_id": b.id, "note_title": n.title,
                                "icon": n.icon, "workspace_id": workspace, "text": text,
                                "stamp": stamp, "emailed": emailed,
                            }));
                        }
                    }
                }
            }
            Ok(json!({"enabled": mail::enabled(), "reminders": mine, "from_notes": from_notes}))
        }
        ("POST", ["reminders"]) => {
            if d.reminders.iter().filter(|r| r.user_id == u.id).count() >= LIMIT {
                return Err(bad("You have too many reminders"));
            }
            let r = if v["note_id"].is_string() {
                if !mail::enabled() {
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
                Reminder {
                    id: id(),
                    user_id: u.id.clone(),
                    note_id,
                    block_id,
                    at,
                    workspace_id: String::new(),
                    title: String::new(),
                    category: String::new(),
                    repeat: None,
                    offset: 0,
                    sent: false,
                }
            } else {
                let workspace_id = field(v, "workspace_id")?;
                if !writes_workspace(d, u, &workspace_id) {
                    return Err(forbidden());
                }
                if d.reminders
                    .iter()
                    .filter(|r| r.workspace_id == workspace_id)
                    .count()
                    >= WORKSPACE_LIMIT
                {
                    return Err(bad("This workspace has too many reminders"));
                }
                let (title, category, at, repeat, offset) = details(v)?;
                Reminder {
                    id: id(),
                    user_id: u.id.clone(),
                    note_id: String::new(),
                    block_id: String::new(),
                    at,
                    workspace_id,
                    title,
                    category,
                    repeat,
                    offset,
                    sent: false,
                }
            };
            d.reminders.push(r.clone());
            mail::wake_sender();
            Ok(view(d, u, &r))
        }
        ("PUT", ["reminders", rid]) => {
            let (title, category, at, repeat, offset) = details(v)?;
            let i = d
                .reminders
                .iter()
                .position(|r| r.id == *rid && r.note_id.is_empty() && may_change(d, u, r))
                .ok_or_else(forbidden)?;
            let r = &mut d.reminders[i];
            if at != r.at {
                r.sent = false;
            }
            (r.title, r.category, r.at, r.repeat, r.offset) = (title, category, at, repeat, offset);
            let r = r.clone();
            mail::wake_sender();
            Ok(view(d, u, &r))
        }
        ("POST", ["reminders", rid, "done"]) => {
            let i = d
                .reminders
                .iter()
                .position(|r| r.id == *rid && may_change(d, u, r))
                .ok_or_else(forbidden)?;
            let r = &mut d.reminders[i];
            let Some(repeat) = r.repeat else {
                d.reminders.remove(i);
                return Ok(json!({"next": null}));
            };
            // Done late, it still keeps its rhythm: the next time after now.
            let mut at = next(r.at, repeat, r.offset);
            for _ in 0..10_000 {
                if at > now() {
                    break;
                }
                at = next(at, repeat, r.offset);
            }
            r.at = at;
            r.sent = false;
            let r = r.clone();
            Ok(json!({"next": view(d, u, &r)}))
        }
        ("DELETE", ["reminders", rid]) => {
            let i = d
                .reminders
                .iter()
                .position(|r| r.id == *rid && may_change(d, u, r))
                .ok_or_else(forbidden)?;
            d.reminders.remove(i);
            Ok(json!({"ok":true}))
        }
        _ => Err(missing()),
    }
}

/// Emails the reminders that are due. A note reminder whose block is gone, or
/// no longer holds a `⏰` time, was removed by the person, so it is dropped.
/// Returns true when anything changed.
pub(crate) fn fire(d: &mut Data) -> bool {
    let t = now();
    if !d.reminders.iter().any(|r| r.at <= t && !r.sent) {
        return false;
    }
    let (due, rest): (Vec<Reminder>, Vec<Reminder>) =
        d.reminders.drain(..).partition(|r| r.at <= t && !r.sent);
    d.reminders = rest;
    for mut r in due {
        if r.note_id.is_empty() {
            let subject = if r.category.is_empty() {
                format!("Reminder: {}", r.title)
            } else {
                format!("Reminder: {} · {}", r.title, r.category)
            };
            let members: Vec<User> = d
                .users
                .iter()
                .filter(|a| !a.user.disabled && sees_workspace(d, &a.user, &r.workspace_id))
                .map(|a| a.user.clone())
                .collect();
            let place = d
                .workspaces
                .iter()
                .find(|w| w.id == r.workspace_id)
                .map_or(String::new(), |w| w.name.clone());
            for user in members {
                let mut lines = vec![
                    format!("Hi {},", user.name),
                    format!("It is time for “{}”, in {place}.", r.title),
                ];
                if let Some(repeat) = r.repeat {
                    lines.push(format!(
                        "It repeats {}. Mark it done on the Reminders page to move it to {}.",
                        every(repeat),
                        date(next(r.at, repeat, r.offset), r.offset)
                    ));
                }
                let link = format!("/?view=reminders&workspace={}", r.workspace_id);
                mail::send(
                    d,
                    &user.email,
                    &subject,
                    &lines,
                    None,
                    ("Open Reminders", &link),
                );
            }
            // Kept until it is marked done, also when it could not be sent.
            r.sent = true;
            d.reminders.push(r);
            continue;
        }
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
        mail::send(
            d,
            &user.email,
            &format!("Reminder: {}", note.title),
            &[
                format!("Hi {},", user.name),
                format!("You asked to be reminded of this, in “{}”:", note.title),
            ],
            Some(&text),
            ("Open the note", &format!("/?note={}", note.id)),
        );
    }
    true
}

/// The local day of `at`, like 2027-10-30.
fn date(at: u64, offset: i32) -> String {
    let (y, m, d) = civil((at as i64 + i64::from(offset) * 60).div_euclid(86_400));
    format!("{y}-{m:02}-{d:02}")
}
/// "every month", "every 2 years".
fn every(r: Repeat) -> String {
    let unit = match r.unit {
        Unit::Day => "day",
        Unit::Week => "week",
        Unit::Month => "month",
        Unit::Year => "year",
    };
    if r.every == 1 {
        format!("every {unit}")
    } else {
        format!("every {} {unit}s", r.every)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(y: i64, m: u32, day: u32, hour: i64) -> u64 {
        (days_from_civil(y, m, day) * 86_400 + hour * 3600) as u64
    }
    #[test]
    fn the_calendar_round_trips() {
        for days in [-800_000, -1, 0, 59, 10_957, 20_000, 2_000_000] {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(month_length(2028, 2), 29);
        assert_eq!(month_length(2100, 2), 28);
    }
    #[test]
    fn repeats_keep_the_day_and_time() {
        let month = Repeat {
            every: 1,
            unit: Unit::Month,
        };
        let years = Repeat {
            every: 2,
            unit: Unit::Year,
        };
        let week = Repeat {
            every: 1,
            unit: Unit::Week,
        };
        assert_eq!(next(at(2026, 1, 31, 9), month, 0), at(2026, 2, 28, 9));
        assert_eq!(next(at(2026, 12, 15, 9), month, 0), at(2027, 1, 15, 9));
        assert_eq!(next(at(2028, 2, 29, 9), years, 0), at(2030, 2, 28, 9));
        assert_eq!(next(at(2026, 10, 30, 9), week, 0), at(2026, 11, 6, 9));
        // 00:30 on 1 March in UTC+2 is 22:30 on 28 February in UTC; a month
        // later is 00:30 on 1 April there, not 28 March.
        let local = at(2026, 2, 28, 22) + 1800;
        assert_eq!(next(local, month, 120), at(2026, 3, 31, 22) + 1800);
    }
    #[test]
    fn finds_times_written_in_blocks() {
        assert_eq!(
            stamps("Call the garage ⏰2026-11-02 09:00 and ⏰2027-01-01T10:30 ⏰soon"),
            vec!["2026-11-02 09:00", "2027-01-01 10:30"]
        );
    }
}
