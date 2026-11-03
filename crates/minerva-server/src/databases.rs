use super::*;
use std::collections::BTreeMap;
pub(super) fn validate_columns(cols: &[Column]) -> Result<(), ApiError> {
    if cols.is_empty() || cols.len() > 32 {
        return Err(bad("A database needs 1–32 properties"));
    }
    let mut ids = HashSet::new();
    for c in cols {
        if c.id.is_empty()
            || c.id.len() > 80
            || !ids.insert(&c.id)
            || c.name.trim().is_empty()
            || c.name.len() > 100
            || c.options.len() > 50
            || c.options.iter().any(|s| s.is_empty() || s.len() > 100)
        {
            return Err(bad("Invalid property schema"));
        }
        let unique: HashSet<_> = c.options.iter().collect();
        if unique.len() != c.options.len() {
            return Err(bad("Duplicate select option"));
        }
        if matches!(c.kind, ColumnKind::Select | ColumnKind::MultiSelect) && c.options.is_empty() {
            return Err(bad("Select properties require options"));
        }
        if (c.kind == ColumnKind::Relation) != c.target.is_some() {
            return Err(bad("A relation needs the database it links to"));
        }
        match (&c.kind, &c.rollup) {
            (ColumnKind::Rollup, Some(r)) => {
                if !cols
                    .iter()
                    .any(|x| x.id == r.relation && x.kind == ColumnKind::Relation)
                {
                    return Err(bad(&format!(
                        "“{}” needs a relation property of this database",
                        c.name
                    )));
                }
                if !formulas::ROLLUP_FUNCTIONS.contains(&r.function.as_str()) {
                    return Err(bad("Unknown rollup calculation"));
                }
                if r.function != "count" && r.property.is_empty() {
                    return Err(bad(&format!("Choose the property “{}” combines", c.name)));
                }
            }
            (ColumnKind::Rollup, None) => {
                return Err(bad("A rollup needs a relation and a calculation"))
            }
            (_, Some(_)) => return Err(bad("Invalid property schema")),
            _ => {}
        }
        if c.kind == ColumnKind::Formula {
            let expr = formulas::parse(&c.formula)
                .map_err(|e| bad(&format!("Formula “{}”: {e}", c.name)))?;
            let mut names = vec![];
            formulas::references(&expr, &mut names);
            for n in names {
                if !cols.iter().any(|x| {
                    x.name.eq_ignore_ascii_case(&n) || x.name.to_lowercase() == n.to_lowercase()
                }) {
                    return Err(bad(&format!(
                        "Formula “{}”: there is no property “{n}”",
                        c.name
                    )));
                }
            }
        } else if !c.formula.is_empty() {
            return Err(bad("Invalid property schema"));
        }
    }
    Ok(())
}
pub(super) fn valid_date(s: &str) -> bool {
    let parts: Vec<_> = s.split('-').collect();
    if parts.len() != 3
        || s.len() != 10
        || parts.iter().map(|p| p.len()).collect::<Vec<_>>() != [4, 2, 2]
        || parts.iter().any(|p| !p.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let nums: Vec<_> = parts.iter().map(|s| s.parse::<u32>()).collect();
    if let (Ok(y), Ok(m), Ok(day)) = (&nums[0], &nums[1], &nums[2]) {
        let days = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        };
        *y >= 1 && *day >= 1 && *day <= days
    } else {
        false
    }
}
pub(super) fn cell_valid(c: &Column, value: &Value) -> bool {
    if value.is_null() {
        return true;
    }
    match c.kind {
        ColumnKind::Text => value.as_str().is_some_and(|s| s.len() <= 10_000),
        ColumnKind::Number => value.as_f64().is_some_and(f64::is_finite),
        ColumnKind::Checkbox => value.is_boolean(),
        ColumnKind::Date => value.as_str().is_some_and(valid_date),
        ColumnKind::Select => value
            .as_str()
            .is_some_and(|s| c.options.iter().any(|o| o == s)),
        ColumnKind::Url => value.as_str().is_some_and(|s| {
            s.len() <= 2048
                && (s.starts_with("https://") || s.starts_with("http://"))
                && !s.chars().any(char::is_whitespace)
                && url::Url::parse(s).is_ok_and(|u| u.host_str().is_some())
        }),
        ColumnKind::MultiSelect => value.as_array().is_some_and(|items| {
            let mut seen = HashSet::new();
            items.iter().all(|v| {
                v.as_str()
                    .is_some_and(|s| c.options.iter().any(|o| o == s) && seen.insert(s))
            })
        }),
        ColumnKind::Person => value.as_str().is_some_and(id_like),
        ColumnKind::Relation => value.as_array().is_some_and(|items| {
            let mut seen = HashSet::new();
            items.len() <= 100
                && items
                    .iter()
                    .all(|v| v.as_str().is_some_and(|s| id_like(s) && seen.insert(s)))
        }),
        // Kept or computed by the server, never written.
        ColumnKind::Created | ColumnKind::Edited | ColumnKind::Rollup | ColumnKind::Formula => {
            false
        }
    }
}
fn id_like(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
/// Relations must link to a database in the same workspace that the person can
/// read. `cols` are the columns to check; `all` the database's columns after the
/// change (rollups find their relation there).
fn validate_targets(
    d: &Data,
    u: &User,
    folder: &str,
    cols: &[Column],
    all: &[Column],
) -> Result<(), ApiError> {
    let workspace = |f: &str| {
        d.folders
            .iter()
            .find(|x| x.id == f)
            .map(|x| &x.workspace_id)
    };
    // A rollup combines a plain property of the related database.
    for c in cols.iter().filter(|c| c.kind == ColumnKind::Rollup) {
        let Some(spec) = &c.rollup else { continue };
        if spec.function == "count" && spec.property.is_empty() {
            continue;
        }
        let target = all
            .iter()
            .find(|x| x.id == spec.relation)
            .and_then(|x| x.target.as_ref())
            .and_then(|t| d.databases.iter().find(|db| &db.id == t));
        let fits = target
            .and_then(|t| t.columns.iter().find(|x| x.id == spec.property))
            .is_some_and(|p| {
                !matches!(
                    p.kind,
                    ColumnKind::Rollup | ColumnKind::Formula | ColumnKind::Relation
                )
            });
        if !fits {
            return Err(bad(&format!(
                "“{}” combines a property of the related database (not a relation, rollup or formula)",
                c.name
            )));
        }
    }
    for c in cols.iter().filter(|c| c.kind == ColumnKind::Relation) {
        let target = c.target.as_deref().unwrap_or("");
        let db = d
            .databases
            .iter()
            .find(|db| db.id == target)
            .ok_or_else(|| bad("The related database does not exist"))?;
        if workspace(&db.folder_id) != workspace(folder)
            || access(d, u, &db.folder_id, false).is_err()
        {
            return Err(bad(
                "A relation links to a database of this workspace that you can open",
            ));
        }
    }
    Ok(())
}
/// A record's name: its first column's text.
pub(super) fn record_title(db: &Database, r: &Record) -> String {
    db.columns
        .first()
        .and_then(|c| r.cells.get(&c.id))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Untitled")
        .to_string()
}
fn validate_cells(cols: &[Column], cells: &BTreeMap<String, Value>) -> Result<(), ApiError> {
    for (key, value) in cells {
        let c = cols
            .iter()
            .find(|c| c.id == *key)
            .ok_or_else(|| bad("Unknown property"))?;
        if !cell_valid(c, value) {
            return Err(bad(&format!("Invalid value for {}", c.name)));
        }
    }
    Ok(())
}
/// A Markdown table of a database's current records, for exports read by `u`:
/// people by name, and linked records by name when `u` can open their database.
pub(super) fn markdown_table(d: &Data, u: &User, db: &Database) -> String {
    let date = |at: u64| {
        let days = (at / 86400) as i64;
        // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!("{year:04}-{month:02}-{day:02}")
    };
    let computed = formulas::computed(d, u, db);
    let row_cell = |c: &Column, r: &Record| -> String {
        match c.kind {
            ColumnKind::Rollup | ColumnKind::Formula => computed
                .get(&r.id)
                .and_then(|values| values.get(&c.id))
                .map(|v| match v {
                    Value::Bool(b) => (if *b { "☑" } else { "☐" }).into(),
                    v => formulas::text(v),
                })
                .unwrap_or_default(),
            ColumnKind::Created => date(if r.created_at > 0 {
                r.created_at
            } else {
                r.updated_at
            }),
            ColumnKind::Edited => date(r.updated_at),
            ColumnKind::Person => r
                .cells
                .get(&c.id)
                .and_then(Value::as_str)
                .and_then(|id| d.users.iter().find(|a| a.user.id == id))
                .map(|a| a.user.name.clone())
                .unwrap_or_default(),
            ColumnKind::Relation => {
                let target = d
                    .databases
                    .iter()
                    .find(|t| Some(&t.id) == c.target.as_ref())
                    .filter(|t| access(d, u, &t.folder_id, false).is_ok());
                r.cells
                    .get(&c.id)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| {
                        let t = target?;
                        t.rows
                            .iter()
                            .find(|x| Some(x.id.as_str()) == id.as_str())
                            .map(|x| record_title(t, x))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            ColumnKind::MultiSelect => r
                .cells
                .get(&c.id)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            _ => match r.cells.get(&c.id) {
                None | Some(Value::Null) => String::new(),
                Some(Value::Bool(b)) => (if *b { "☑" } else { "☐" }).into(),
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
            },
        }
    };
    let cell = |text: String| text.replace('|', "\\|").replace(['\n', '\r'], " ");
    let mut out = format!("**{}**\n\n", db.name.replace(['*', '\n'], ""));
    out.push_str(&format!(
        "| {} |\n",
        db.columns
            .iter()
            .map(|c| cell(c.name.clone()))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    out.push_str(&format!("|{}\n", " --- |".repeat(db.columns.len())));
    for row in db.rows.iter().filter(|r| !r.archived) {
        out.push_str(&format!(
            "| {} |\n",
            db.columns
                .iter()
                .map(|c| cell(row_cell(c, row)))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    out
}
/// A database as `u` reads it: each record with its `computed` rollup and
/// formula values.
pub(super) fn view(d: &Data, u: &User, db: &Database) -> Value {
    let mut computed = formulas::computed(d, u, db);
    let mut v = json!(db);
    if let Some(rows) = v["rows"].as_array_mut() {
        for row in rows {
            let id = row["id"].as_str().unwrap_or_default().to_string();
            row["computed"] = json!(computed.remove(&id).unwrap_or_default());
            if let Some(page) = record_pages::page_of(d, &db.id, &id).filter(|p| !p.trashed) {
                row["page"] = json!(page.id);
            }
        }
    }
    v
}
/// One record of `did` as `u` reads it, with its computed values.
fn record_view(d: &Data, u: &User, did: &str, rid: &str) -> Value {
    let Some(db) = d.databases.iter().find(|db| db.id == did) else {
        return Value::Null;
    };
    let mut computed = formulas::computed(d, u, db);
    let mut v = json!(db.rows.iter().find(|r| r.id == rid));
    v["computed"] = json!(computed.remove(rid).unwrap_or_default());
    if let Some(page) = record_pages::page_of(d, did, rid).filter(|p| !p.trashed) {
        v["page"] = json!(page.id);
    }
    v
}
pub(super) fn route(
    d: &mut Data,
    u: &User,
    method: &str,
    parts: &[&str],
    v: Value,
) -> Result<Value, ApiError> {
    if parts == ["databases"] && method == "POST" {
        let folder_id = field(&v, "folder_id")?;
        access(d, u, &folder_id, true)?;
        let cols = if v["columns"].is_array() {
            serde_json::from_value(v["columns"].clone()).map_err(|_| bad("Invalid properties"))?
        } else {
            vec![
                Column {
                    id: id(),
                    name: "Name".into(),
                    kind: ColumnKind::Text,
                    options: vec![],
                    target: None,
                    rollup: None,
                    formula: String::new(),
                },
                Column {
                    id: id(),
                    name: "Status".into(),
                    kind: ColumnKind::Select,
                    options: vec!["To do".into(), "In progress".into(), "Done".into()],
                    target: None,
                    rollup: None,
                    formula: String::new(),
                },
            ]
        };
        validate_columns(&cols)?;
        validate_targets(d, u, &folder_id, &cols, &cols)?;
        let db = Database {
            id: id(),
            folder_id,
            name: field(&v, "name")?,
            columns: cols,
            rows: vec![],
            revision: 1,
            updated_at: now(),
        };
        d.databases.push(db.clone());
        return Ok(json!(db));
    }
    let did = parts.get(1).ok_or_else(missing)?;
    let db = d
        .databases
        .iter()
        .find(|db| db.id == *did)
        .ok_or_else(missing)?;
    access(d, u, &db.folder_id, method != "GET")?;
    // The people a person property can name: members who can open the database.
    if method == "GET" && parts.len() == 3 && parts[2] == "people" {
        let folder = db.folder_id.clone();
        let mut people: Vec<Value> = d
            .users
            .iter()
            .filter(|a| !a.user.disabled && !a.user.admin && role(d, &a.user, &folder).is_some())
            .map(|a| json!({"id": a.user.id, "name": a.user.name}))
            .collect();
        people.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_lowercase());
        return Ok(json!(people));
    }
    if method == "DELETE" && parts.len() == 2 {
        // Notes that embed the database show that it was deleted; its records'
        // pages go with it.
        d.databases.retain(|db| db.id != *did);
        let pages = record_pages::orphans(d, did);
        removal::remove_notes(d, &pages);
        return Ok(json!({"ok":true}));
    }
    if method == "POST" && parts.len() == 5 && parts[2] == "rows" && parts[4] == "page" {
        return record_pages::open(d, u, did, parts[3]);
    }
    let db = d.databases.iter_mut().find(|db| db.id == *did).unwrap();
    match (method, parts.len()) {
        ("GET", 2) => {
            let db = db.clone();
            Ok(view(d, u, &db))
        }
        ("DELETE", 4) if parts[2] == "rows" => {
            let before = db.rows.len();
            db.rows.retain(|r| r.id != parts[3]);
            if db.rows.len() == before {
                return Err(missing());
            }
            db.updated_at = now();
            let pages = record_pages::orphans(d, did);
            removal::remove_notes(d, &pages);
            Ok(json!({"ok":true}))
        }
        ("PATCH", 2) => {
            if v["revision"].as_u64() != Some(db.revision) {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Database schema changed. Reload before editing.".into(),
                ));
            }
            let mut columns: Vec<Column> = if v["columns"].is_array() {
                serde_json::from_value(v["columns"].clone())
                    .map_err(|_| bad("Invalid properties"))?
            } else {
                db.columns.clone()
            };
            // Formulas follow renamed properties.
            for old in &db.columns {
                let Some(new) = columns
                    .iter()
                    .find(|c| c.id == old.id)
                    .map(|c| c.name.clone())
                else {
                    continue;
                };
                if new != old.name {
                    for c in columns.iter_mut().filter(|c| c.kind == ColumnKind::Formula) {
                        c.formula = formulas::rename(&c.formula, &old.name, &new);
                    }
                }
            }
            validate_columns(&columns)?;
            // Relations that are new or changed must link to a database the person
            // can open; one they cannot open does not block other edits.
            let folder = db.folder_id.clone();
            let changed: Vec<Column> = columns
                .iter()
                .filter(|c| {
                    !db.columns
                        .iter()
                        .any(|o| o.id == c.id && o.target == c.target && o.rollup == c.rollup)
                })
                .cloned()
                .collect();
            validate_targets(d, u, &folder, &changed, &columns)?;
            let db = d.databases.iter_mut().find(|db| db.id == *did).unwrap();
            if v.get("name").is_some() {
                db.name = field(&v, "name")?;
            }
            // Removing a property, a select option, or changing a property's type
            // clears the values that no longer fit, rather than refusing the change.
            for row in &mut db.rows {
                // A multi-select keeps the options that remain.
                for c in columns.iter().filter(|c| c.kind == ColumnKind::MultiSelect) {
                    if let Some(Value::Array(items)) = row.cells.get_mut(&c.id) {
                        let before = items.len();
                        items.retain(|v| {
                            v.as_str().is_some_and(|s| c.options.iter().any(|o| o == s))
                        });
                        if items.len() != before {
                            row.revision += 1;
                            row.updated_at = now();
                        }
                    }
                }
                let before = row.cells.len();
                row.cells.retain(|key, value| {
                    columns
                        .iter()
                        .find(|c| c.id == *key)
                        .is_some_and(|c| cell_valid(c, value))
                });
                if row.cells.len() != before {
                    row.revision += 1;
                    row.updated_at = now();
                }
            }
            db.columns = columns;
            db.revision += 1;
            db.updated_at = now();
            let db = db.clone();
            // Pages follow their records' names (the first property may have changed).
            for r in &db.rows {
                record_pages::retitle(d, did, &r.id)?;
            }
            Ok(view(d, u, &db))
        }
        ("POST", 3) if parts[2] == "rows" => {
            if db.rows.len() >= 10000 {
                return Err(bad("Database record limit reached"));
            }
            let cells = serde_json::from_value(v["cells"].clone())
                .map_err(|_| bad("Expected cells object"))?;
            validate_cells(&db.columns, &cells)?;
            let row = Record {
                id: id(),
                cells,
                revision: 1,
                updated_at: now(),
                created_at: now(),
                archived: false,
                conflict_of: None,
            };
            db.rows.push(row.clone());
            db.updated_at = now();
            Ok(record_view(d, u, did, &row.id))
        }
        ("PATCH", 4) if parts[2] == "rows" => {
            let row = db
                .rows
                .iter()
                .find(|r| r.id == parts[3])
                .ok_or_else(missing)?;
            if v["revision"].as_u64() != Some(row.revision) {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "This record changed. Reload to see the latest values.".into(),
                ));
            }
            let mut cells = row.cells.clone();
            if let Some(changes) = v["cells"].as_object() {
                for (k, val) in changes {
                    cells.insert(k.clone(), val.clone());
                }
            }
            validate_cells(&db.columns, &cells)?;
            let row = db.rows.iter_mut().find(|r| r.id == parts[3]).unwrap();
            row.cells = cells;
            if let Some(archived) = v["archived"].as_bool() {
                row.archived = archived;
            }
            row.revision += 1;
            row.updated_at = now();
            db.updated_at = now();
            record_pages::retitle(d, did, parts[3])?;
            Ok(record_view(d, u, did, parts[3]))
        }
        _ => Err(missing()),
    }
}
