//! User settings (`settings.json` in the config directory). Missing file or fields mean the defaults.

use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub struct Settings {
    /// The Space the window works in (`None`: General).
    #[serde(default)]
    pub active_space: Option<u64>,
    /// Show what is in every Space, not only General and the Space in use.
    #[serde(default)]
    pub view_all: bool,
    /// `"terminal"` (pastels on your terminal's own background, the default) or `"classic"` (fixed colours).
    #[serde(default = "terminal")]
    pub theme: String,
}

fn terminal() -> String {
    "terminal".into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings { theme: terminal(), active_space: None, view_all: false }
    }
}

impl Settings {
    pub fn load() -> Self {
        Self::load_from(&crate::store::Store::config_dir())
    }

    pub fn load_from(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join("settings.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
}

/// Remembers one setting of `settings.json` in `dir`, keeping the rest of the file.
pub fn save_key_in(dir: &Path, key: &str, value: serde_json::Value) {
    let path = dir.join("settings.json");
    let mut file = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    file[key] = value;
    if let Ok(text) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(path, text);
    }
}

/// Remembers the theme, keeping the rest of the file.
pub fn save_theme(theme: &str) {
    let path = crate::store::Store::config_dir().join("settings.json");
    let mut value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    value["theme"] = serde_json::Value::String(theme.into());
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(path, text);
    }
}
