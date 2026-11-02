//! `minerva-cli`: a few everyday actions against a Minerva server, signed in
//! with a personal API token (Settings → API tokens).
//!
//! ```text
//! export MINERVA_URL=https://notes.example.com MINERVA_TOKEN=mnv_api_v1_…
//! minerva-cli search budget review
//! minerva-cli folders
//! echo "- call the bank" | minerva-cli new Inbox "Monday"
//! minerva-cli append <note-id> "One more thing"
//! minerva-cli backup ./minerva-backup
//! ```
use serde_json::{json, Value};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "Usage: minerva-cli <command>

  search <words…>              Notes matching the words, best first
  folders                      Folders you can open, with their IDs
  new <folder> <title> [text]  A new note; the folder by ID or name. The text
                               (Markdown) is the argument or standard input
  append <note-id> [text]      Adds Markdown to the end of a note
  backup <directory>           Saves every note you can open as Markdown (with
                               --files: a zip with its attachments) and every
                               database as JSON, by workspace and folder

Environment: MINERVA_URL (the server's address) and MINERVA_TOKEN (a personal
API token, created in Settings → API tokens).";

struct Client {
    base: url::Url,
    token: String,
    http: reqwest::blocking::Client,
}
impl Client {
    fn from_env() -> Result<Self, String> {
        let base = std::env::var("MINERVA_URL")
            .map_err(|_| "Set MINERVA_URL to the server's address".to_string())?;
        let token = std::env::var("MINERVA_TOKEN")
            .map_err(|_| "Set MINERVA_TOKEN to a personal API token".to_string())?;
        let mut base = url::Url::parse(base.trim())
            .map_err(|_| format!("MINERVA_URL is not an address: {base}"))?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        if base.scheme() != "https"
            && !matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
        {
            return Err("Use an https:// address (http only for localhost)".into());
        }
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("minerva-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            base,
            token: token.trim().into(),
            http,
        })
    }
    fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<reqwest::blocking::Response, String> {
        let url = self.base.join(path).map_err(|e| e.to_string())?;
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(&self.token)
            .header("x-minerva-client", "1");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .map_err(|e| format!("Cannot reach the server: {e}"))?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let message = response
            .json::<Value>()
            .ok()
            .and_then(|v| v["error"].as_str().map(String::from))
            .unwrap_or_else(|| status.to_string());
        Err(if status == reqwest::StatusCode::UNAUTHORIZED {
            format!("{message} (check MINERVA_TOKEN)")
        } else {
            message
        })
    }
    fn get(&self, path: &str) -> Result<Value, String> {
        self.send(reqwest::Method::GET, &format!("api/v1/{path}"), None)?
            .json()
            .map_err(|e| e.to_string())
    }
    fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        self.send(reqwest::Method::POST, &format!("api/v1/{path}"), Some(body))?
            .json()
            .map_err(|e| e.to_string())
    }
}

/// The text argument, or standard input when there is none and it is not a terminal.
fn text(args: &[String]) -> Result<String, String> {
    if !args.is_empty() {
        return Ok(args.join(" "));
    }
    let mut input = String::new();
    if !std::io::stdin().is_terminal() {
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
    }
    Ok(input)
}
fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map_or(&[], Vec::as_slice)
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
/// "Workspace / Parent / Folder".
fn folder_path(boot: &Value, folder: &Value) -> String {
    let workspace = list(boot, "workspaces")
        .iter()
        .find(|w| w["id"] == folder["workspace_id"])
        .map_or("?", |w| s(w, "name"));
    let mut names = vec![s(folder, "name").to_string()];
    let mut parent = folder["parent_id"].clone();
    while let Some(p) = list(boot, "folders").iter().find(|f| f["id"] == parent) {
        if names.len() > 20 {
            break;
        }
        names.insert(0, s(p, "name").to_string());
        parent = p["parent_id"].clone();
    }
    names.insert(0, workspace.to_string());
    names.join(" / ")
}
/// A folder by ID, or by name or path when exactly one matches.
fn find_folder<'a>(boot: &'a Value, wanted: &str) -> Result<&'a Value, String> {
    let folders = list(boot, "folders");
    if let Some(f) = folders.iter().find(|f| s(f, "id") == wanted) {
        return Ok(f);
    }
    let lower = wanted.to_lowercase();
    let found: Vec<&Value> = folders
        .iter()
        .filter(|f| {
            s(f, "name").to_lowercase() == lower || folder_path(boot, f).to_lowercase() == lower
        })
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("No folder “{wanted}”; see `minerva-cli folders`")),
        many => Err(format!(
            "Several folders are called “{wanted}”; use an ID:\n{}",
            many.iter()
                .map(|f| format!("  {}  {}", s(f, "id"), folder_path(boot, f)))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}
/// A file or directory name: no separators or control characters.
fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean
        .trim()
        .trim_start_matches('.')
        .chars()
        .take(120)
        .collect::<String>();
    if clean.is_empty() {
        "Untitled".into()
    } else {
        clean
    }
}
/// `dir/name.ext`, or with the ID added if that is taken.
fn unique(dir: &Path, name: &str, id: &str, ext: &str) -> PathBuf {
    let path = dir.join(format!("{}.{ext}", file_name(name)));
    if path.exists() {
        dir.join(format!(
            "{} ({}).{ext}",
            file_name(name),
            &id[..id.len().min(8)]
        ))
    } else {
        path
    }
}

fn backup(c: &Client, dir: &Path, files: bool) -> Result<(), String> {
    let boot = c.get("bootstrap")?;
    let folder_dir = |fid: &Value| -> Option<PathBuf> {
        let f = list(&boot, "folders").iter().find(|f| f["id"] == *fid)?;
        Some(
            folder_path(&boot, f)
                .split(" / ")
                .fold(dir.to_path_buf(), |p, part| p.join(file_name(part))),
        )
    };
    let (mut notes, mut databases, mut failed) = (0, 0, 0);
    for n in list(&boot, "notes").iter().filter(|n| n["trashed"] != true) {
        let Some(target) = folder_dir(&n["folder_id"]) else {
            continue;
        };
        std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        let format = if files { "zip" } else { "md" };
        let id = s(n, "id");
        let result = c
            .send(
                reqwest::Method::GET,
                &format!("export/{id}?format={format}"),
                None,
            )
            .and_then(|r| r.bytes().map_err(|e| e.to_string()));
        match result {
            Ok(bytes) => {
                let path = unique(&target, s(n, "title"), id, format);
                std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                notes += 1;
            }
            Err(e) => {
                eprintln!("Skipped “{}”: {e}", s(n, "title"));
                failed += 1;
            }
        }
    }
    for db in list(&boot, "databases") {
        let Some(target) = folder_dir(&db["folder_id"]) else {
            continue;
        };
        std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        match c.get(&format!("databases/{}", s(db, "id"))) {
            Ok(full) => {
                let path = unique(&target, s(db, "name"), s(db, "id"), "database.json");
                let text = serde_json::to_string_pretty(&full).map_err(|e| e.to_string())?;
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                databases += 1;
            }
            Err(e) => {
                eprintln!("Skipped database “{}”: {e}", s(db, "name"));
                failed += 1;
            }
        }
    }
    println!(
        "Saved {notes} notes and {databases} databases in {}",
        dir.display()
    );
    if failed > 0 {
        return Err(format!("{failed} could not be saved"));
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(USAGE.into());
    };
    if matches!(command, "-h" | "--help" | "help") {
        println!("{USAGE}");
        return Ok(());
    }
    let rest = &args[1..];
    let c = Client::from_env()?;
    match command {
        "search" if !rest.is_empty() => {
            let found = c.post("search", json!({"query": rest.join(" ")}))?;
            let found = found.as_array().map_or(&[][..], Vec::as_slice);
            if found.is_empty() {
                println!("Nothing found.");
            }
            for m in found {
                println!("{}  {}", s(m, "id"), s(m, "title"));
                let snippet = s(m, "snippet").replace(['\u{1}', '\u{2}'], "");
                let snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
                if !snippet.is_empty() {
                    println!("    {snippet}");
                }
            }
            Ok(())
        }
        "folders" => {
            let boot = c.get("bootstrap")?;
            let mut rows: Vec<(String, String)> = list(&boot, "folders")
                .iter()
                .map(|f| (folder_path(&boot, f), s(f, "id").to_string()))
                .collect();
            rows.sort();
            for (path, id) in rows {
                println!("{id}  {path}");
            }
            Ok(())
        }
        "new" if rest.len() >= 2 => {
            let boot = c.get("bootstrap")?;
            let folder = find_folder(&boot, &rest[0])?;
            let note = c.post(
                "notes",
                json!({"folder_id": folder["id"], "title": rest[1], "markdown": text(&rest[2..])?}),
            )?;
            println!("{}", s(&note, "id"));
            Ok(())
        }
        "append" if !rest.is_empty() => {
            let markdown = text(&rest[1..])?;
            if markdown.trim().is_empty() {
                return Err("Nothing to append: give text or pipe it in".into());
            }
            let note = c.post(
                &format!("notes/{}/append", rest[0]),
                json!({"markdown": markdown}),
            )?;
            println!("Added to “{}”", s(&note, "title"));
            Ok(())
        }
        "backup" if !rest.is_empty() => {
            let files = rest.iter().any(|a| a == "--files");
            let dir = rest.iter().find(|a| *a != "--files").ok_or(USAGE)?;
            backup(&c, Path::new(dir), files)
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_paths_and_lookup() {
        let boot = json!({
            "workspaces": [{"id":"w","name":"Team"}],
            "folders": [
                {"id":"a","workspace_id":"w","name":"Projects"},
                {"id":"b","workspace_id":"w","name":"Inbox","parent_id":"a"},
                {"id":"c","workspace_id":"w","name":"Inbox"},
            ],
        });
        assert_eq!(
            folder_path(&boot, &boot["folders"][1]),
            "Team / Projects / Inbox"
        );
        assert_eq!(find_folder(&boot, "projects").unwrap()["id"], "a");
        assert_eq!(find_folder(&boot, "b").unwrap()["id"], "b");
        assert_eq!(
            find_folder(&boot, "Team / Projects / Inbox").unwrap()["id"],
            "b"
        );
        assert!(find_folder(&boot, "Inbox").unwrap_err().contains("Several"));
        assert!(find_folder(&boot, "Nope").is_err());
    }
    #[test]
    fn safe_file_names() {
        assert_eq!(file_name("a/b: c?"), "a_b_ c_");
        assert_eq!(file_name("../.."), "_..");
        assert_eq!(file_name("  "), "Untitled");
    }
}
