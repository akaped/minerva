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
        if c.kind == ColumnKind::Select && c.options.is_empty() {
            return Err(bad("Select properties require options"));
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
    }
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
/// A Markdown table of a database's current records, for exports.
pub(super) fn markdown_table(db: &Database) -> String {
    let cell = |c: &Column, v: Option<&Value>| -> String {
        let text = match (&c.kind, v) {
            (_, None | Some(Value::Null)) => String::new(),
            (ColumnKind::Checkbox, Some(Value::Bool(b))) => (if *b { "☑" } else { "☐" }).into(),
            (_, Some(Value::String(s))) => s.clone(),
            (_, Some(other)) => other.to_string(),
        };
        text.replace('|', "\\|").replace(['\n', '\r'], " ")
    };
    let mut out = format!("**{}**\n\n", db.name.replace(['*', '\n'], ""));
    out.push_str(&format!(
        "| {} |\n",
        db.columns
            .iter()
            .map(|c| cell(c, Some(&json!(c.name))))
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    out.push_str(&format!("|{}\n", " --- |".repeat(db.columns.len())));
    for row in db.rows.iter().filter(|r| !r.archived) {
        out.push_str(&format!(
            "| {} |\n",
            db.columns
                .iter()
                .map(|c| cell(c, row.cells.get(&c.id)))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    out
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
                },
                Column {
                    id: id(),
                    name: "Status".into(),
                    kind: ColumnKind::Select,
                    options: vec!["To do".into(), "In progress".into(), "Done".into()],
                },
            ]
        };
        validate_columns(&cols)?;
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
    if method == "DELETE" && parts.len() == 2 {
        // Notes that embed the database show that it was deleted.
        d.databases.retain(|db| db.id != *did);
        return Ok(json!({"ok":true}));
    }
    let db = d.databases.iter_mut().find(|db| db.id == *did).unwrap();
    match (method, parts.len()) {
        ("GET", 2) => Ok(json!(db)),
        ("DELETE", 4) if parts[2] == "rows" => {
            let before = db.rows.len();
            db.rows.retain(|r| r.id != parts[3]);
            if db.rows.len() == before {
                return Err(missing());
            }
            db.updated_at = now();
            Ok(json!({"ok":true}))
        }
        ("PATCH", 2) => {
            if v["revision"].as_u64() != Some(db.revision) {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Database schema changed. Reload before editing.".into(),
                ));
            }
            let columns: Vec<Column> = if v["columns"].is_array() {
                serde_json::from_value(v["columns"].clone())
                    .map_err(|_| bad("Invalid properties"))?
            } else {
                db.columns.clone()
            };
            validate_columns(&columns)?;
            if v.get("name").is_some() {
                db.name = field(&v, "name")?;
            }
            // Removing a property, a select option, or changing a property's type
            // clears the values that no longer fit, rather than refusing the change.
            for row in &mut db.rows {
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
            Ok(json!(db))
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
                archived: false,
                conflict_of: None,
            };
            db.rows.push(row.clone());
            db.updated_at = now();
            Ok(json!(row))
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
            Ok(json!(row))
        }
        _ => Err(missing()),
    }
}
