//! Journals, templates, and unfinished journal tasks.
//!
//! - A workspace's Journal folder holds one note per date (`Note::journal`).
//! - A workspace's Templates folder holds notes that can be copied into empty
//!   notes, with `{{date}}`, `{{weekday}}`, `{{long_date}}`, and `{{title}}` filled in.
//! - `[[uncompleted_journal_tasks]]` in a note lists open tasks from earlier journal
//!   entries, computed for each reader.
use super::*;

pub(super) const UNFINISHED_TASKS: &str = "[[uncompleted_journal_tasks]]";

#[derive(Clone, Copy)]
enum Special {
    Journal,
    Templates,
}

/// Days since 1970-01-01 for a valid `YYYY-MM-DD` (Howard Hinnant's algorithm).
fn days(date: &str) -> i64 {
    let y: i64 = date[0..4].parse().unwrap_or(1970);
    let m: i64 = date[5..7].parse().unwrap_or(1);
    let d: i64 = date[8..10].parse().unwrap_or(1);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn weekday(date: &str) -> &'static str {
    [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ][days(date).rem_euclid(7) as usize]
}
fn long_date(date: &str) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month = MONTHS[date[5..7].parse::<usize>().unwrap_or(1).clamp(1, 12) - 1];
    format!(
        "{}, {month} {}, {}",
        weekday(date),
        date[8..10].trim_start_matches('0'),
        &date[0..4]
    )
}
fn date_field(v: &Value, key: &str) -> Result<String, ApiError> {
    let date = v[key].as_str().unwrap_or("").to_string();
    if databases::valid_date(&date) {
        Ok(date)
    } else {
        Err(bad("Dates use the YYYY-MM-DD format"))
    }
}
fn workspace_of<'a>(d: &'a Data, folder_id: &str) -> Option<&'a str> {
    d.folders
        .iter()
        .find(|f| f.id == folder_id)
        .map(|f| f.workspace_id.as_str())
}
fn visible_workspace(d: &Data, u: &User, ws: &str) -> Result<(), ApiError> {
    if !d.workspaces.iter().any(|w| w.id == ws) {
        return Err(missing());
    }
    if owns(d, u, ws)
        || d.folders
            .iter()
            .any(|f| f.workspace_id == ws && role(d, u, &f.id).is_some())
    {
        Ok(())
    } else {
        Err(forbidden())
    }
}
fn special_folder_id(d: &Data, ws: &str, kind: Special) -> Option<String> {
    let map = match kind {
        Special::Journal => &d.journal_folders,
        Special::Templates => &d.template_folders,
    };
    map.get(ws)
        .filter(|f| {
            d.folders
                .iter()
                .any(|folder| &&folder.id == f && folder.workspace_id == ws)
        })
        .cloned()
}
/// Returns the workspace's special folder, creating it when the user can edit the workspace.
fn special_folder(d: &mut Data, u: &User, ws: &str, kind: Special) -> Result<String, ApiError> {
    if let Some(folder) = special_folder_id(d, ws, kind) {
        return Ok(folder);
    }
    let name = match kind {
        Special::Journal => "Journal",
        Special::Templates => "Templates",
    };
    if !edits_workspace(d, u, ws) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            format!("Someone who can edit this workspace must open {name} once to set it up"),
        ));
    }
    let folder = Folder {
        id: id(),
        workspace_id: ws.to_string(),
        name: name.into(),
        parent_id: None,
    };
    let fid = folder.id.clone();
    d.folders.push(folder);
    match kind {
        Special::Journal => d.journal_folders.insert(ws.to_string(), fid.clone()),
        Special::Templates => d.template_folders.insert(ws.to_string(), fid.clone()),
    };
    Ok(fid)
}
fn is_template(d: &Data, n: &Note) -> bool {
    workspace_of(d, &n.folder_id).is_some_and(|ws| {
        special_folder_id(d, ws, Special::Templates).as_deref() == Some(n.folder_id.as_str())
    })
}
fn view(d: &Data, u: &User, n: &Note) -> Value {
    let mut v = note_view(n);
    v["role"] = json!(note_role(d, u, n));
    v["sharing"] = sharing::summary(d, &n.id);
    v
}
/// Template blocks with variables filled in and fresh block IDs.
fn placeholders(text: &str, title: &str, date: &str) -> String {
    text.replace("{{date}}", date)
        .replace("{{weekday}}", weekday(date))
        .replace("{{long_date}}", &long_date(date))
        .replace("{{title}}", title)
}
fn fill(template: &Note, title: &str, date: &str) -> Vec<Block> {
    template
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| Block::new(&placeholders(&b.markdown, title, date), i))
        .collect()
}
fn save(d: &mut Data, updated: Note) -> Result<(), ApiError> {
    let index = d
        .notes
        .iter()
        .position(|n| n.id == updated.id)
        .ok_or_else(missing)?;
    let previous = d.notes[index].clone();
    let history = d.history.entry(updated.id.clone()).or_default();
    history.push(previous);
    if history.len() > 50 {
        history.remove(0);
    }
    let mut updated = updated;
    updated.revision += 1;
    updated.updated_at = now();
    d.notes[index] = updated.clone();
    collaboration::replace_note(d, &updated)
}

pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: &Value,
) -> Result<Option<Value>, ApiError> {
    let result = match (method, parts) {
        ("POST", ["journal"]) => open_day(d, u, v)?,
        ("PUT", ["journal", "settings"]) => {
            let ws = field(v, "workspace_id")?;
            if !edits_workspace(d, u, &ws) {
                return Err(forbidden());
            }
            match v["template_id"].as_str().filter(|t| !t.is_empty()) {
                Some(template) => {
                    let n = d
                        .notes
                        .iter()
                        .find(|n| n.id == template)
                        .ok_or_else(missing)?;
                    if !is_template(d, n) || workspace_of(d, &n.folder_id) != Some(ws.as_str()) {
                        return Err(bad("Choose a template from this workspace"));
                    }
                    d.journal_templates.insert(ws.clone(), template.to_string());
                }
                None => {
                    d.journal_templates.remove(&ws);
                }
            }
            json!({"template_id": d.journal_templates.get(&ws)})
        }
        ("POST", ["templates"]) => {
            let ws = field(v, "workspace_id")?;
            visible_workspace(d, u, &ws)?;
            let folder = special_folder(d, u, &ws, Special::Templates)?;
            access(d, u, &folder, true)?;
            let note = Note {
                id: id(),
                folder_id: folder,
                title: field(v, "title")?,
                icon: "✦".into(),
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
            d.notes.push(note.clone());
            view(d, u, &note)
        }
        ("POST", ["notes", nid, "template"]) => {
            let target = d
                .notes
                .iter()
                .find(|n| n.id == *nid)
                .ok_or_else(missing)?
                .clone();
            note_access(d, u, &target, true)?;
            let template = d
                .notes
                .iter()
                .find(|n| Some(n.id.as_str()) == v["template_id"].as_str())
                .ok_or_else(missing)?
                .clone();
            if !is_template(d, &template) || template.trashed {
                return Err(bad("That note is not a template"));
            }
            note_access(d, u, &template, false)?;
            if target.blocks.iter().any(|b| !b.markdown.trim().is_empty()) {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Templates fill empty notes only".into(),
                ));
            }
            let date = target
                .journal
                .clone()
                .map_or_else(|| date_field(v, "date"), Ok)?;
            let mut updated = target.clone();
            updated.blocks = fill(&template, &target.title, &date);
            // Templates also carry document metadata and a PDF template.
            for (key, value) in &template.metadata {
                updated
                    .metadata
                    .entry(key.clone())
                    .or_insert_with(|| placeholders(value, &target.title, &date));
            }
            if updated.print_template.is_empty() {
                updated.print_template = template.print_template.clone();
            }
            if matches!(target.icon.as_str(), "▤" | "") {
                updated.icon = template.icon.clone();
            }
            if target.cover.is_empty() {
                updated.cover = template.cover.clone();
            }
            for tag in &template.tags {
                if !updated.tags.contains(tag) && updated.tags.len() < 20 {
                    updated.tags.push(tag.clone());
                }
            }
            save(d, updated)?;
            let saved = d.notes.iter().find(|n| n.id == *nid).unwrap().clone();
            view(d, u, &saved)
        }
        ("POST", ["journal", "tasks"]) => {
            let (context, scan) = scan_scope(d, u, v)?;
            json!(open_tasks(d, &context, &scan))
        }
        ("POST", ["journal", "tasks", "complete"]) => {
            let (_, scan) = scan_scope(d, u, v)?;
            let target = key(v["text"].as_str().unwrap_or(""));
            if target.is_empty() {
                return Err(bad("Missing task"));
            }
            let mut completed = 0;
            for nid in scan {
                let n = d.notes.iter().find(|n| n.id == nid).unwrap().clone();
                if note_access(d, u, &n, true).is_err() {
                    continue;
                }
                let mut updated = n.clone();
                let mut changed = false;
                for block in &mut updated.blocks {
                    let lines: Vec<String> = block
                        .markdown
                        .split('\n')
                        .map(|line| match task(line) {
                            Some((false, text)) if key(text) == target => {
                                changed = true;
                                completed += 1;
                                line.replacen("[ ]", "[x]", 1)
                            }
                            _ => line.to_string(),
                        })
                        .collect();
                    block.markdown = lines.join("\n");
                }
                if changed {
                    save(d, updated)?;
                }
            }
            json!({"completed": completed})
        }
        _ => return Ok(None),
    };
    Ok(Some(result))
}

fn open_day(d: &mut Data, u: &User, v: &Value) -> Result<Value, ApiError> {
    let ws = field(v, "workspace_id")?;
    let date = date_field(v, "date")?;
    visible_workspace(d, u, &ws)?;
    let journal_folder = special_folder_id(d, &ws, Special::Journal);
    if let Some(existing) = d
        .notes
        .iter()
        .find(|n| {
            n.journal.as_deref() == Some(date.as_str())
                && Some(&n.folder_id) == journal_folder.as_ref()
        })
        .cloned()
    {
        note_access(d, u, &existing, false)?;
        let mut result = view(d, u, &existing);
        result["created"] = json!(false);
        return Ok(result);
    }
    let folder = special_folder(d, u, &ws, Special::Journal)?;
    access(d, u, &folder, true)?;
    let template = d
        .journal_templates
        .get(&ws)
        .and_then(|t| {
            d.notes
                .iter()
                .find(|n| &n.id == t && !n.trashed && is_template(d, n))
        })
        .filter(|t| note_access(d, u, t, false).is_ok())
        .cloned();
    let blocks = template
        .as_ref()
        .map_or_else(|| vec![Block::new("", 0)], |t| fill(t, &date, &date));
    let note = Note {
        id: id(),
        folder_id: folder,
        title: date.clone(),
        icon: "🗓".into(),
        blocks,
        revision: 1,
        updated_at: now(),
        favorite: false,
        trashed: false,
        cover: template
            .as_ref()
            .map(|t| t.cover.clone())
            .unwrap_or_default(),
        tags: template
            .as_ref()
            .map(|t| t.tags.clone())
            .unwrap_or_default(),
        metadata: template
            .as_ref()
            .map(|t| {
                t.metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), placeholders(v, &date, &date)))
                    .collect()
            })
            .unwrap_or_default(),
        print_template: template
            .as_ref()
            .map(|t| t.print_template.clone())
            .unwrap_or_default(),
        journal: Some(date),
    };
    d.notes.push(note.clone());
    let mut result = view(d, u, &note);
    result["created"] = json!(true);
    Ok(result)
}

/// A Markdown task line: `- [ ] text` / `- [x] text` (also `*` and `+`).
fn task(line: &str) -> Option<(bool, &str)> {
    let trimmed = line.trim_start();
    let rest = ["- ", "* ", "+ "]
        .iter()
        .find_map(|m| trimmed.strip_prefix(m))?
        .trim_start();
    let (done, text) = if let Some(text) = rest.strip_prefix("[ ]") {
        (false, text)
    } else {
        (
            true,
            rest.strip_prefix("[x]")
                .or_else(|| rest.strip_prefix("[X]"))?,
        )
    };
    let text = text.trim();
    (!text.is_empty()).then_some((done, text))
}
/// Tasks are the same task when their text matches ignoring case and spacing.
fn key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
/// The context note and the journal notes (oldest first) whose tasks it gathers:
/// entries before the context note's date, or up to `today` for other notes.
fn scan_scope(d: &Data, u: &User, v: &Value) -> Result<(Note, Vec<String>), ApiError> {
    let context = d
        .notes
        .iter()
        .find(|n| Some(n.id.as_str()) == v["note_id"].as_str())
        .ok_or_else(missing)?
        .clone();
    note_access(d, u, &context, false)?;
    let today = date_field(v, "today")?;
    let Some(folder) = workspace_of(d, &context.folder_id)
        .and_then(|ws| special_folder_id(d, ws, Special::Journal))
    else {
        return Ok((context, vec![]));
    };
    let mut entries: Vec<&Note> = d
        .notes
        .iter()
        .filter(|n| {
            n.folder_id == folder
                && !n.trashed
                && n.id != context.id
                && note_access(d, u, n, false).is_ok()
        })
        .filter(|n| match (&n.journal, &context.journal) {
            (Some(date), Some(cutoff)) => date < cutoff,
            (Some(date), None) => date <= &today,
            _ => false,
        })
        .collect();
    entries.sort_by(|a, b| a.journal.cmp(&b.journal));
    Ok((context, entries.into_iter().map(|n| n.id.clone()).collect()))
}
/// Open tasks, oldest origin first. A later entry completing a task removes it;
/// repeats collapse to their earliest occurrence; tasks already in the context
/// note are omitted.
fn open_tasks(d: &Data, context: &Note, scan: &[String]) -> Vec<Value> {
    let present: HashSet<String> = context
        .blocks
        .iter()
        .flat_map(|b| b.markdown.split('\n'))
        .filter_map(task)
        .map(|(_, text)| key(text))
        .collect();
    let mut order: Vec<String> = vec![];
    let mut open: HashMap<String, Value> = HashMap::new();
    for nid in scan {
        let n = d.notes.iter().find(|n| &n.id == nid).unwrap();
        for block in &n.blocks {
            for line in block.markdown.split('\n') {
                let Some((done, text)) = task(line) else {
                    continue;
                };
                let k = key(text);
                if done {
                    open.remove(&k);
                } else if !open.contains_key(&k) {
                    let html = render(text);
                    let html = html
                        .trim()
                        .strip_prefix("<p>")
                        .and_then(|h| h.strip_suffix("</p>"))
                        .unwrap_or(&html)
                        .to_string();
                    open.insert(
                        k.clone(),
                        json!({"text": text, "html": html, "note_id": n.id, "block_id": block.id, "date": n.journal}),
                    );
                    order.push(k);
                }
            }
        }
    }
    let mut seen = HashSet::new();
    order
        .into_iter()
        .filter(|k| !present.contains(k) && seen.insert(k.clone()))
        .filter_map(|k| open.remove(&k))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_tasks_and_keys() {
        assert_eq!(weekday("2026-10-24"), "Saturday");
        assert_eq!(weekday("1970-01-01"), "Thursday");
        assert_eq!(weekday("2000-02-29"), "Tuesday");
        assert_eq!(long_date("2026-03-05"), "Thursday, March 5, 2026");
        assert_eq!(task("- [ ] Call Ana"), Some((false, "Call Ana")));
        assert_eq!(task("  * [X] done"), Some((true, "done")));
        assert_eq!(task("- [ ]"), None);
        assert_eq!(task("- plain"), None);
        assert_eq!(key("  Call   ANA "), "call ana");
    }
}
