//! Profile details are returned only to the account's own session.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};

const PRESETS: &[&str] = &["fox", "frog", "robot", "alien", "octopus", "owl"];
const PNG_PREFIX: &str = "data:image/png;base64,";
const MAX_PNG_BASE64: usize = 96 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Profile {
    first_name: String,
    last_name: String,
    avatar: Option<String>,
}

impl Profile {
    pub(super) fn view(&self, name: &str) -> Value {
        if !self.first_name.is_empty() && self.display_name() == name {
            return json!(self);
        }
        let (first_name, last_name) = name.split_once(' ').unwrap_or((name, ""));
        json!({"first_name": first_name, "last_name": last_name, "avatar": self.avatar})
    }

    pub(super) fn parse(value: &Value) -> Result<Self, ApiError> {
        let name_part = |key, required| {
            let part = value[key].as_str().ok_or_else(invalid_name)?.trim();
            if (required && part.is_empty())
                || part.chars().count() > 200
                || part.chars().any(char::is_control)
            {
                return Err(invalid_name());
            }
            Ok(part.to_owned())
        };
        let first_name = name_part("first_name", true)?;
        let last_name = name_part("last_name", false)?;
        let avatar = match value.get("avatar") {
            None | Some(Value::Null) => None,
            Some(Value::String(avatar)) if valid_avatar(avatar) => Some(avatar.clone()),
            _ => return Err(bad("Choose an avatar or upload a valid profile image")),
        };
        Ok(Self {
            first_name,
            last_name,
            avatar,
        })
    }

    pub(super) fn display_name(&self) -> String {
        if self.last_name.is_empty() {
            self.first_name.clone()
        } else {
            format!("{} {}", self.first_name, self.last_name)
        }
    }
}

fn invalid_name() -> ApiError {
    bad("Enter a first name and use at most 200 characters per name")
}

fn valid_avatar(avatar: &str) -> bool {
    if let Some(preset) = avatar.strip_prefix("preset:") {
        return PRESETS.contains(&preset);
    }
    let Some(encoded) = avatar.strip_prefix(PNG_PREFIX) else {
        return false;
    };
    if encoded.len() > MAX_PNG_BASE64 {
        return false;
    }
    let Ok(bytes) = STANDARD.decode(encoded) else {
        return false;
    };
    let mut decoder = png::Decoder::new_with_limits(
        std::io::Cursor::new(bytes),
        png::Limits { bytes: 512 * 1024 },
    );
    decoder.set_ignore_text_chunk(true);
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let info = reader.info();
    if info.width > 128 || info.height > 128 || info.animation_control.is_some() {
        return false;
    }
    let Some(size) = reader.output_buffer_size() else {
        return false;
    };
    reader.next_frame(&mut vec![0; size]).is_ok()
}
