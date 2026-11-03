//! Computed database properties. A **formula** is a small expression over the
//! record's other properties, such as `prop("Price") * prop("Quantity")` or
//! `if(prop("Done"), "✓", days(today(), prop("Due")) + " days left")`. A
//! **rollup** combines a property of the records a relation links to: count,
//! sum, average, earliest… Both are computed when a database is read, for the
//! person reading it (a rollup only sees databases they can open), and never
//! stored in cells.
//!
//! Values are JSON: numbers, text, booleans, lists, `null`; dates are
//! `YYYY-MM-DD` text. An expression that does not fit its values (text times a
//! number, division by zero) gives `null` rather than an error.
use super::*;
use std::collections::BTreeMap;

const MAX_LENGTH: usize = 1000;
const MAX_DEPTH: usize = 40;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Expr {
    Value(Value),
    Prop(String),
    Call(String, Vec<Expr>),
    Unary(char, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}

const FUNCTIONS: [(&str, usize, usize); 22] = [
    // name, fewest and most arguments
    ("if", 3, 3),
    ("and", 1, 20),
    ("or", 1, 20),
    ("not", 1, 1),
    ("empty", 1, 1),
    ("concat", 1, 20),
    ("join", 2, 2),
    ("length", 1, 1),
    ("contains", 2, 2),
    ("lower", 1, 1),
    ("upper", 1, 1),
    ("round", 1, 2),
    ("floor", 1, 1),
    ("ceil", 1, 1),
    ("abs", 1, 1),
    ("min", 1, 20),
    ("max", 1, 20),
    ("today", 0, 0),
    ("days", 2, 2),
    ("add_days", 2, 2),
    ("year", 1, 1),
    ("month", 1, 1),
];

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Text(String),
    Name(String),
    Op(String),
    Open,
    Close,
    Comma,
}

fn tokens(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            out.push(Token::Number(
                text.parse()
                    .map_err(|_| format!("“{text}” is not a number"))?,
            ));
        } else if c == '"' || c == '\'' {
            let mut text = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err("A text in quotes is not closed".into()),
                    Some(&q) if q == c => break,
                    Some('\\') if chars.get(i + 1).is_some() => {
                        text.push(chars[i + 1]);
                        i += 2;
                    }
                    Some(&x) => {
                        text.push(x);
                        i += 1;
                    }
                }
            }
            i += 1;
            out.push(Token::Text(text));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else if c == '(' {
            out.push(Token::Open);
            i += 1;
        } else if c == ')' {
            out.push(Token::Close);
            i += 1;
        } else if c == ',' {
            out.push(Token::Comma);
            i += 1;
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            let op = if ["==", "!=", "<=", ">=", "&&", "||"].contains(&two.as_str()) {
                two
            } else if "+-*/%<>=!".contains(c) {
                c.to_string()
            } else {
                return Err(format!("“{c}” cannot be used here"));
            };
            i += op.chars().count();
            out.push(Token::Op(op));
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}
impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }
    fn next(&mut self) -> Option<Token> {
        self.at += 1;
        self.tokens.get(self.at - 1).cloned()
    }
    fn op(&mut self, ops: &[&str]) -> Option<String> {
        let found = match self.peek() {
            Some(Token::Op(o)) if ops.contains(&o.as_str()) => Some(o.clone()),
            Some(Token::Name(n)) if ops.contains(&n.to_lowercase().as_str()) => {
                Some(n.to_lowercase())
            }
            _ => None,
        };
        if found.is_some() {
            self.at += 1;
        }
        found
    }
    fn binary(
        &mut self,
        ops: &[&str],
        next: fn(&mut Self) -> Result<Expr, String>,
    ) -> Result<Expr, String> {
        let mut left = next(self)?;
        while let Some(op) = self.op(ops) {
            let op = match op.as_str() {
                "&&" => "and".into(),
                "||" => "or".into(),
                "=" => "==".into(),
                _ => op,
            };
            left = Expr::Binary(op, Box::new(left), Box::new(next(self)?));
        }
        Ok(left)
    }
    fn expr(&mut self) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("The formula is nested too deeply".into());
        }
        let e = self.binary(&["or", "||"], |p| {
            p.binary(&["and", "&&"], |p| {
                p.binary(&["==", "=", "!=", "<", "<=", ">", ">="], |p| {
                    p.binary(&["+", "-"], |p| p.binary(&["*", "/", "%"], Self::unary))
                })
            })
        });
        self.depth -= 1;
        e
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if let Some(op) = self.op(&["-", "!", "not"]) {
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err("The formula is nested too deeply".into());
            }
            let inner = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Unary(
                if op == "-" { '-' } else { '!' },
                Box::new(inner),
            ));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Token::Number(n)) => Ok(Expr::Value(json!(n))),
            Some(Token::Text(t)) => Ok(Expr::Value(json!(t))),
            Some(Token::Open) => {
                let e = self.expr()?;
                match self.next() {
                    Some(Token::Close) => Ok(e),
                    _ => Err("A “(” is not closed".into()),
                }
            }
            Some(Token::Name(n)) if matches!(n.to_lowercase().as_str(), "true" | "false") => {
                Ok(Expr::Value(json!(n.eq_ignore_ascii_case("true"))))
            }
            Some(Token::Name(n)) if self.peek() == Some(&Token::Open) => {
                let n = n.to_lowercase();
                self.at += 1;
                let mut args = vec![];
                if self.peek() == Some(&Token::Close) {
                    self.at += 1;
                } else {
                    loop {
                        args.push(self.expr()?);
                        match self.next() {
                            Some(Token::Comma) => continue,
                            Some(Token::Close) => break,
                            _ => return Err(format!("Expected “,” or “)” in {n}(…)")),
                        }
                    }
                }
                if n == "prop" {
                    return match args.as_slice() {
                        [Expr::Value(Value::String(name))] => Ok(Expr::Prop(name.clone())),
                        _ => Err("prop(…) takes a property name in quotes".into()),
                    };
                }
                let (_, fewest, most) = FUNCTIONS
                    .iter()
                    .find(|(f, _, _)| *f == n)
                    .ok_or_else(|| format!("There is no function “{n}”"))?;
                if args.len() < *fewest || args.len() > *most {
                    return Err(format!(
                        "{n}(…) takes {}",
                        if fewest == most {
                            format!("{fewest} values")
                        } else {
                            format!("{fewest}–{most} values")
                        }
                    ));
                }
                Ok(Expr::Call(n, args))
            }
            Some(Token::Name(n)) => Err(format!(
                "Unknown name “{n}”; properties are written prop(\"{n}\")"
            )),
            Some(t) => Err(format!("Unexpected {}", describe(&t))),
            None => Err("The formula ends too early".into()),
        }
    }
}
fn describe(t: &Token) -> String {
    match t {
        Token::Op(o) => format!("“{o}”"),
        Token::Close => "“)”".into(),
        Token::Comma => "“,”".into(),
        _ => "value".into(),
    }
}

pub(super) fn parse(src: &str) -> Result<Expr, String> {
    if src.trim().is_empty() {
        return Err("Write a formula".into());
    }
    if src.len() > MAX_LENGTH {
        return Err("A formula has up to 1,000 characters".into());
    }
    let mut p = Parser {
        tokens: tokens(src)?,
        at: 0,
        depth: 0,
    };
    let e = p.expr()?;
    match p.peek() {
        None => Ok(e),
        Some(t) => Err(format!("Unexpected {} after the formula", describe(t))),
    }
}
/// The property names a formula refers to.
pub(super) fn references(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Prop(n) => out.push(n.clone()),
        Expr::Call(_, args) => args.iter().for_each(|a| references(a, out)),
        Expr::Unary(_, a) => references(a, out),
        Expr::Binary(_, a, b) => {
            references(a, out);
            references(b, out);
        }
        Expr::Value(_) => {}
    }
}
/// The formula with `prop("old")` renamed (any case) to `prop("new")`.
pub(super) fn rename(src: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(at) = rest.find("prop(") {
        out.push_str(&rest[..at + 5]);
        rest = &rest[at + 5..];
        let trimmed = rest.trim_start();
        let Some(quote) = trimmed.chars().next().filter(|q| *q == '"' || *q == '\'') else {
            continue;
        };
        let body = &trimmed[1..];
        if let Some(end) = body.find(quote) {
            if body[..end].eq_ignore_ascii_case(old)
                || body[..end].to_lowercase() == old.to_lowercase()
            {
                out.push_str(&rest[..rest.len() - trimmed.len()]);
                out.push('"');
                out.push_str(&new.replace('\\', "\\\\").replace('"', "\\\""));
                out.push('"');
                rest = &body[end + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// Evaluation

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(_) => true,
    }
}
/// Text for joining: numbers without needless decimals, lists with commas.
pub(super) fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => number_text(n.as_f64().unwrap_or(0.0)),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        Value::Object(_) => String::new(),
    }
}
fn number_text(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let s = format!("{:.6}", x);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
fn number(x: f64) -> Value {
    if x.is_finite() {
        json!(if x.fract() == 0.0 && x.abs() < 1e15 {
            json!(x as i64)
        } else {
            json!(x)
        })
    } else {
        Value::Null
    }
}
/// Days since 1970-01-01 of a `YYYY-MM-DD` date.
fn day_number(v: &Value) -> Option<i64> {
    let s = v.as_str()?;
    if !databases::valid_date(s) {
        return None;
    }
    let (y, m, d) = (
        s[0..4].parse::<i64>().ok()?,
        s[5..7].parse::<i64>().ok()?,
        s[8..10].parse::<i64>().ok()?,
    );
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}
pub(super) fn date_text(days: i64) -> String {
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
}
fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64()?.partial_cmp(&y.as_f64()?),
        (Value::String(x), Value::String(y)) => Some(x.to_lowercase().cmp(&y.to_lowercase())),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}
fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::String(x), Value::String(y)) => x.to_lowercase() == y.to_lowercase(),
        _ => a == b,
    }
}

/// The values a formula sees, by lowercase property name.
pub(super) type Scope<'a> = &'a dyn Fn(&str) -> Option<Value>;

pub(super) fn eval(e: &Expr, scope: Scope, today: i64) -> Value {
    let ev = |x: &Expr| eval(x, scope, today);
    let num = |x: &Expr| ev(x).as_f64();
    match e {
        Expr::Value(v) => v.clone(),
        Expr::Prop(name) => scope(&name.to_lowercase()).unwrap_or(Value::Null),
        Expr::Unary('-', a) => num(a).map_or(Value::Null, |x| number(-x)),
        Expr::Unary(_, a) => json!(!truthy(&ev(a))),
        Expr::Binary(op, a, b) => {
            if op == "and" {
                return json!(truthy(&ev(a)) && truthy(&ev(b)));
            }
            if op == "or" {
                return json!(truthy(&ev(a)) || truthy(&ev(b)));
            }
            let (x, y) = (ev(a), ev(b));
            match op.as_str() {
                "==" => json!(equal(&x, &y)),
                "!=" => json!(!equal(&x, &y)),
                "<" | "<=" | ">" | ">=" => compare(&x, &y).map_or(Value::Null, |o| {
                    json!(match op.as_str() {
                        "<" => o.is_lt(),
                        "<=" => o.is_le(),
                        ">" => o.is_gt(),
                        _ => o.is_ge(),
                    })
                }),
                // `+` joins when either side is text.
                "+" if x.is_string() || y.is_string() => json!(text(&x) + &text(&y)),
                _ => match (x.as_f64(), y.as_f64()) {
                    (Some(x), Some(y)) => number(match op.as_str() {
                        "+" => x + y,
                        "-" => x - y,
                        "*" => x * y,
                        "/" if y != 0.0 => x / y,
                        "%" if y != 0.0 => x % y,
                        _ => f64::NAN,
                    }),
                    _ => Value::Null,
                },
            }
        }
        Expr::Call(f, args) => call(f, args, scope, today),
    }
}
fn call(f: &str, args: &[Expr], scope: Scope, today: i64) -> Value {
    let ev = |i: usize| eval(&args[i], scope, today);
    let all = || {
        args.iter()
            .map(|a| eval(a, scope, today))
            .collect::<Vec<_>>()
    };
    let nums = || -> Vec<f64> {
        all()
            .into_iter()
            .flat_map(|v| match v {
                Value::Array(items) => items,
                v => vec![v],
            })
            .filter_map(|v| v.as_f64())
            .collect()
    };
    match f {
        "if" => {
            if truthy(&ev(0)) {
                ev(1)
            } else {
                ev(2)
            }
        }
        "and" => json!(args.iter().all(|a| truthy(&eval(a, scope, today)))),
        "or" => json!(args.iter().any(|a| truthy(&eval(a, scope, today)))),
        "not" => json!(!truthy(&ev(0))),
        "empty" => json!(!truthy(&ev(0)) && ev(0) != json!(false) && ev(0) != json!(0)),
        "concat" => json!(all().iter().map(text).collect::<String>()),
        "join" => match ev(0) {
            Value::Array(items) => json!(items
                .iter()
                .map(text)
                .collect::<Vec<_>>()
                .join(&text(&ev(1)))),
            v => json!(text(&v)),
        },
        "length" => match ev(0) {
            Value::Array(items) => json!(items.len()),
            Value::Null => json!(0),
            v => json!(text(&v).chars().count()),
        },
        "contains" => {
            let needle = text(&ev(1)).to_lowercase();
            match ev(0) {
                Value::Array(items) => {
                    json!(items.iter().any(|i| text(i).to_lowercase() == needle))
                }
                v => json!(text(&v).to_lowercase().contains(&needle)),
            }
        }
        "lower" => json!(text(&ev(0)).to_lowercase()),
        "upper" => json!(text(&ev(0)).to_uppercase()),
        "round" => {
            let digits = if args.len() > 1 {
                ev(1).as_f64().unwrap_or(0.0).clamp(0.0, 10.0)
            } else {
                0.0
            };
            let scale = 10f64.powi(digits as i32);
            ev(0)
                .as_f64()
                .map_or(Value::Null, |x| number((x * scale).round() / scale))
        }
        "floor" => ev(0).as_f64().map_or(Value::Null, |x| number(x.floor())),
        "ceil" => ev(0).as_f64().map_or(Value::Null, |x| number(x.ceil())),
        "abs" => ev(0).as_f64().map_or(Value::Null, |x| number(x.abs())),
        "min" | "max" => {
            let values = nums();
            let pick = if f == "min" {
                values.iter().copied().reduce(f64::min)
            } else {
                values.iter().copied().reduce(f64::max)
            };
            pick.map_or(Value::Null, number)
        }
        "today" => json!(date_text(today)),
        "days" => match (day_number(&ev(0)), day_number(&ev(1))) {
            (Some(a), Some(b)) => json!(b - a),
            _ => Value::Null,
        },
        "add_days" => match (day_number(&ev(0)), ev(1).as_f64()) {
            (Some(d), Some(n)) if n.abs() < 1e6 => json!(date_text(d + n as i64)),
            _ => Value::Null,
        },
        "year" => ev(0)
            .as_str()
            .filter(|s| databases::valid_date(s))
            .map_or(Value::Null, |s| json!(s[0..4].parse::<i64>().unwrap_or(0))),
        "month" => ev(0)
            .as_str()
            .filter(|s| databases::valid_date(s))
            .map_or(Value::Null, |s| json!(s[5..7].parse::<i64>().unwrap_or(0))),
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Rollups

/// How a rollup combines the related records' values.
pub(super) const ROLLUP_FUNCTIONS: [&str; 9] = [
    "count",
    "count_values",
    "sum",
    "average",
    "min",
    "max",
    "earliest",
    "latest",
    "list",
];

pub(super) fn rollup(function: &str, values: &[Value], linked: usize) -> Value {
    let present: Vec<&Value> = values
        .iter()
        .filter(|v| truthy(v) || **v == json!(0) || **v == json!(false))
        .collect();
    let numbers: Vec<f64> = present.iter().filter_map(|v| v.as_f64()).collect();
    match function {
        "count" => json!(linked),
        "count_values" => json!(present.len()),
        "sum" => number(numbers.iter().sum()),
        "average" if !numbers.is_empty() => {
            number(numbers.iter().sum::<f64>() / numbers.len() as f64)
        }
        "min" => numbers
            .iter()
            .copied()
            .reduce(f64::min)
            .map_or(Value::Null, number),
        "max" => numbers
            .iter()
            .copied()
            .reduce(f64::max)
            .map_or(Value::Null, number),
        "earliest" | "latest" => {
            let mut dates: Vec<&str> = present
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| databases::valid_date(s))
                .collect();
            dates.sort_unstable();
            let pick = if function == "earliest" {
                dates.first()
            } else {
                dates.last()
            };
            pick.map_or(Value::Null, |d| json!(d))
        }
        "list" => json!(present.iter().map(|v| text(v)).collect::<Vec<_>>()),
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// Values of a database's records, as `u` sees them.

/// A cell as a formula or rollup sees it: people and related records by name,
/// created and edited times as dates.
fn plain(
    c: &Column,
    r: &Record,
    people: &HashMap<String, String>,
    related: &HashMap<String, Option<&Database>>,
) -> Value {
    let day = |at: u64| json!(date_text((at / 86400) as i64));
    match c.kind {
        ColumnKind::Created => day(if r.created_at > 0 {
            r.created_at
        } else {
            r.updated_at
        }),
        ColumnKind::Edited => day(r.updated_at),
        ColumnKind::Person => r
            .cells
            .get(&c.id)
            .and_then(Value::as_str)
            .map_or(Value::Null, |id| {
                json!(people.get(id).cloned().unwrap_or_else(|| "Someone".into()))
            }),
        ColumnKind::Relation => {
            let target = c
                .target
                .as_ref()
                .and_then(|t| related.get(t).copied().flatten());
            let ids = r
                .cells
                .get(&c.id)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            json!(ids
                .iter()
                .filter_map(|id| target?
                    .rows
                    .iter()
                    .find(|x| Some(x.id.as_str()) == id.as_str()))
                .map(|x| databases::record_title(target.unwrap(), x))
                .collect::<Vec<_>>())
        }
        _ => r.cells.get(&c.id).cloned().unwrap_or(Value::Null),
    }
}

/// Computed values of every record: record ID → property ID → value.
pub(super) fn computed(
    d: &Data,
    u: &User,
    db: &Database,
) -> BTreeMap<String, BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    let has_computed = db
        .columns
        .iter()
        .any(|c| matches!(c.kind, ColumnKind::Rollup | ColumnKind::Formula));
    if !has_computed {
        return out;
    }
    let today = (now() / 86400) as i64;
    let people: HashMap<String, String> = d
        .users
        .iter()
        .map(|a| (a.user.id.clone(), a.user.name.clone()))
        .collect();
    // Databases that relations link to, when `u` may read them.
    let mut related: HashMap<String, Option<&Database>> = HashMap::new();
    for c in db.columns.iter().filter(|c| c.kind == ColumnKind::Relation) {
        if let Some(t) = &c.target {
            related.entry(t.clone()).or_insert_with(|| {
                d.databases
                    .iter()
                    .find(|x| &x.id == t && role(d, u, &x.folder_id).is_some())
            });
        }
    }
    let formulas: Vec<(&Column, Option<Expr>)> = db
        .columns
        .iter()
        .filter(|c| c.kind == ColumnKind::Formula)
        .map(|c| (c, parse(&c.formula).ok()))
        .collect();
    for r in &db.rows {
        let mut values: BTreeMap<String, Value> = BTreeMap::new();
        for c in db.columns.iter().filter(|c| c.kind == ColumnKind::Rollup) {
            values.insert(c.id.clone(), rollup_value(db, c, r, &people, &related));
        }
        // Formulas see other formulas; one that refers back to itself gives null.
        let mut busy: Vec<String> = vec![];
        for (c, expr) in &formulas {
            if !values.contains_key(&c.id) {
                let v = formula_value(
                    db,
                    c,
                    expr.as_ref(),
                    r,
                    &people,
                    &related,
                    &formulas,
                    &mut values,
                    &mut busy,
                    today,
                );
                values.insert(c.id.clone(), v);
            }
        }
        out.insert(r.id.clone(), values);
    }
    out
}
fn rollup_value(
    db: &Database,
    c: &Column,
    r: &Record,
    people: &HashMap<String, String>,
    related: &HashMap<String, Option<&Database>>,
) -> Value {
    let Some(spec) = &c.rollup else {
        return Value::Null;
    };
    let Some(relation) = db
        .columns
        .iter()
        .find(|x| x.id == spec.relation && x.kind == ColumnKind::Relation)
    else {
        return Value::Null;
    };
    let Some(target) = relation
        .target
        .as_ref()
        .and_then(|t| related.get(t).copied().flatten())
    else {
        return Value::Null;
    };
    let ids = r
        .cells
        .get(&relation.id)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let linked: Vec<&Record> = ids
        .iter()
        .filter_map(|id| {
            target
                .rows
                .iter()
                .find(|x| Some(x.id.as_str()) == id.as_str())
        })
        .collect();
    let property = target.columns.iter().find(|x| x.id == spec.property);
    let values: Vec<Value> = match property {
        Some(p) => linked
            .iter()
            .map(|x| plain(p, x, people, &HashMap::new()))
            .flat_map(|v| match v {
                Value::Array(items) => items,
                v => vec![v],
            })
            .collect(),
        None => vec![],
    };
    if property.is_none() && spec.function != "count" {
        return Value::Null;
    }
    rollup(&spec.function, &values, linked.len())
}
#[allow(clippy::too_many_arguments)]
fn formula_value(
    db: &Database,
    c: &Column,
    expr: Option<&Expr>,
    r: &Record,
    people: &HashMap<String, String>,
    related: &HashMap<String, Option<&Database>>,
    formulas: &[(&Column, Option<Expr>)],
    values: &mut BTreeMap<String, Value>,
    busy: &mut Vec<String>,
    today: i64,
) -> Value {
    let Some(expr) = expr else { return Value::Null };
    if busy.contains(&c.id) {
        return Value::Null;
    }
    busy.push(c.id.clone());
    // Formulas it refers to first, so the scope below can read them.
    let mut names = vec![];
    references(expr, &mut names);
    for name in names {
        if let Some((other, e)) = formulas
            .iter()
            .find(|(f, _)| f.name.to_lowercase() == name.to_lowercase())
        {
            if !values.contains_key(&other.id) && !busy.contains(&other.id) {
                let v = formula_value(
                    db,
                    other,
                    e.as_ref(),
                    r,
                    people,
                    related,
                    formulas,
                    values,
                    busy,
                    today,
                );
                values.insert(other.id.clone(), v);
            }
        }
    }
    busy.pop();
    let snapshot = values.clone();
    let scope = |name: &str| -> Option<Value> {
        let col = db.columns.iter().find(|x| x.name.to_lowercase() == name)?;
        Some(match col.kind {
            ColumnKind::Rollup | ColumnKind::Formula => {
                snapshot.get(&col.id).cloned().unwrap_or(Value::Null)
            }
            _ => plain(col, r, people, related),
        })
    };
    eval(expr, &scope, today)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(src: &str, props: &[(&str, Value)]) -> Value {
        let e = parse(src).unwrap_or_else(|err| panic!("{src}: {err}"));
        let scope = |n: &str| {
            props
                .iter()
                .find(|(k, _)| k.to_lowercase() == n)
                .map(|(_, v)| v.clone())
        };
        // 2026-11-03
        eval(&e, &scope, day_number(&json!("2026-11-03")).unwrap())
    }
    #[test]
    fn arithmetic_text_and_logic() {
        assert_eq!(run("1 + 2 * 3", &[]), json!(7));
        assert_eq!(run("(1 + 2) * 3 / 2", &[]), json!(4.5));
        assert_eq!(run("10 % 4 - -1", &[]), json!(3));
        assert_eq!(
            run(
                r#"prop("Price") * prop("qty")"#,
                &[("Price", json!(2.5)), ("Qty", json!(4))]
            ),
            json!(10)
        );
        assert_eq!(run(r#""Total: " + 3"#, &[]), json!("Total: 3"));
        assert_eq!(run("1 / 0", &[]), Value::Null);
        assert_eq!(run(r#""a" * 2"#, &[]), Value::Null);
        assert_eq!(
            run(r#"if(prop("Done"), "✓", "…")"#, &[("Done", json!(true))]),
            json!("✓")
        );
        assert_eq!(run("1 < 2 and not (2 == 3) || false", &[]), json!(true));
        assert_eq!(run(r#""Abc" = "aBC""#, &[]), json!(true));
        assert_eq!(run(r#"empty(prop("Missing"))"#, &[]), json!(true));
        assert_eq!(run("round(2 / 3, 2)", &[]), json!(0.67));
        assert_eq!(
            run(r#"concat(upper("a"), length("añb"), lower("C"))"#, &[]),
            json!("A3c")
        );
        assert_eq!(
            run(
                r#"contains(prop("Tags"), "Urgent")"#,
                &[("Tags", json!(["urgent", "x"]))]
            ),
            json!(true)
        );
        assert_eq!(
            run(
                r#"join(prop("Tags"), " / ")"#,
                &[("Tags", json!(["a", "b"]))]
            ),
            json!("a / b")
        );
        assert_eq!(run("max(3, 9, 4) + min(2, 1)", &[]), json!(10));
    }
    #[test]
    fn dates() {
        assert_eq!(run("today()", &[]), json!("2026-11-03"));
        assert_eq!(
            run(
                r#"days(today(), prop("Due"))"#,
                &[("Due", json!("2026-12-25"))]
            ),
            json!(52)
        );
        assert_eq!(
            run(r#"add_days("2024-02-28", 2)"#, &[]),
            json!("2024-03-01")
        );
        assert_eq!(
            run(r#"year("2024-02-28") * 100 + month("2024-02-28")"#, &[]),
            json!(202402)
        );
        assert_eq!(
            run(r#"prop("Due") < today()"#, &[("Due", json!("2026-01-01"))]),
            json!(true)
        );
        for days in [-1000, 0, 19_000, 20_396] {
            assert_eq!(day_number(&json!(date_text(days))), Some(days));
        }
    }
    #[test]
    fn mistakes_are_explained() {
        for (src, says) in [
            ("", "Write a formula"),
            ("1 +", "ends too early"),
            ("(1 + 2", "not closed"),
            ("Price * 2", "prop(\"Price\")"),
            ("nope(1)", "no function"),
            ("if(1, 2)", "takes 3 values"),
            ("prop(Price)", "prop(\"Price\")"),
            ("prop(1)", "prop(…) takes"),
            ("1 2", "after the formula"),
            ("\"open", "not closed"),
            ("1 $ 2", "cannot be used"),
        ] {
            let err = parse(src).unwrap_err();
            assert!(err.contains(says), "{src}: {err}");
        }
        assert!(parse(&"(".repeat(100)).unwrap_err().contains("nested"));
        assert!(parse(&"1+".repeat(600)).is_err());
    }
    #[test]
    fn renaming_properties() {
        assert_eq!(
            rename(
                r#"prop("Price") * prop( 'price' ) + prop("Qty")"#,
                "Price",
                "Cost"
            ),
            r#"prop("Cost") * prop( "Cost" ) + prop("Qty")"#
        );
        assert_eq!(
            rename(r#"prop("A")"#, "A", "Say \"hi\""),
            r#"prop("Say \"hi\"")"#
        );
        assert_eq!(
            parse(&rename(r#"prop("A")"#, "A", "Say \"hi\"")).unwrap(),
            Expr::Prop("Say \"hi\"".into())
        );
    }
    #[test]
    fn rollups() {
        let values = [json!(3), json!(null), json!(4.5), json!(0)];
        assert_eq!(rollup("count", &values, 4), json!(4));
        assert_eq!(rollup("count_values", &values, 4), json!(3));
        assert_eq!(rollup("sum", &values, 4), json!(7.5));
        assert_eq!(rollup("average", &values, 4), json!(2.5));
        assert_eq!(rollup("min", &values, 4), json!(0));
        assert_eq!(rollup("max", &values, 4), json!(4.5));
        assert_eq!(rollup("average", &[], 0), Value::Null);
        let dates = [json!("2026-03-01"), json!("2025-12-31"), json!("bad")];
        assert_eq!(rollup("earliest", &dates, 3), json!("2025-12-31"));
        assert_eq!(rollup("latest", &dates, 3), json!("2026-03-01"));
        assert_eq!(
            rollup("list", &[json!("a"), json!(null), json!(2)], 3),
            json!(["a", "2"])
        );
    }
}
