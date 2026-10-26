//! Versioned wire models shared by the server and future native Rust clients.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub admin: bool,
    pub can_create_workspace: bool,
    pub disabled: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    /// Each account's own "My Workspace": never shared, renamed, or deleted.
    #[serde(default)]
    pub personal: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Folder {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    /// The containing folder, in the same workspace; `None` at the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Editor,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Grant {
    pub folder_id: String,
    pub user_id: String,
    pub role: Role,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Block {
    pub id: String,
    pub markdown: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}
impl Block {
    pub fn new(markdown: &str, index: usize) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            markdown: markdown.into(),
            x: 40.0 + (index % 3) as f64 * 310.0,
            y: 40.0 + (index / 3) as f64 * 180.0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Note {
    pub id: String,
    pub folder_id: String,
    pub title: String,
    pub icon: String,
    pub blocks: Vec<Block>,
    pub revision: u64,
    pub updated_at: u64,
    pub favorite: bool,
    pub trashed: bool,
    /// Cover image: empty, a preset (`gradient:1`–`gradient:8`), an attachment URL
    /// (`/api/v1/attachments/{id}`), or an HTTPS image URL.
    #[serde(default)]
    pub cover: String,
    /// Normalized note tags (lowercase, without `#`). Inline `#tags` in the text are
    /// derived separately.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The date (`YYYY-MM-DD`) of a journal entry; set when the entry is created.
    #[serde(default)]
    pub journal: Option<String>,
    /// Document metadata (`author`, `company`, `version`, ...) used by PDF templates
    /// and `{{key}}` placeholders in exports. Keys are lowercase `[a-z0-9_]`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub metadata: std::collections::BTreeMap<String, String>,
    /// The PDF template (`plugins/pdf-prints`) last chosen for this note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub print_template: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SaveNote {
    pub title: String,
    pub icon: String,
    pub blocks: Vec<Block>,
    pub revision: u64,
    pub favorite: bool,
    pub trashed: bool,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Text,
    Number,
    Checkbox,
    Date,
    Select,
    Url,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Column {
    pub id: String,
    pub name: String,
    pub kind: ColumnKind,
    #[serde(default)]
    pub options: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Record {
    pub id: String,
    pub cells: std::collections::BTreeMap<String, serde_json::Value>,
    pub revision: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub archived: bool,
    /// Set on an archived copy that keeps the losing values of a concurrent offline
    /// edit; names the record it conflicted with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_of: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Database {
    pub id: String,
    pub folder_id: String,
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Record>,
    pub revision: u64,
    pub updated_at: u64,
}
