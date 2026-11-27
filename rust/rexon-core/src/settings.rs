//! Application settings — port of `rexon/settings.py`.
//!
//! JSON store `~/.config/rexon/settings.json`: defaults, merging with the file,
//! get/set, file manager detection. Serialization matches Python
//! `json.dump(..., indent=2, ensure_ascii=False)` byte for byte — see the
//! golden test `tests/settings_parity.rs`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Defaults in the SAME order as `_DEFAULTS` in settings.py — key order
/// is visible in the saved file, so it matters for comparison.
pub fn defaults() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("file_manager".into(), Value::from("auto"));
    m.insert("theme".into(), Value::from("auto"));
    m.insert("auto_backup".into(), Value::from(true));
    m.insert("wizard_done".into(), Value::from(false));
    m.insert("show_console_window".into(), Value::from(true));
    m.insert("ui_skin".into(), Value::from("steam_dark"));
    m
}

/// File managers in priority order (as in settings.py).
const FILE_MANAGERS: [&str; 6] = ["nemo", "nautilus", "thunar", "caja", "dolphin", "pcmanfm"];

/// Merge the defaults with the file content: `dict(_DEFAULTS)` then `.update(json)`.
/// insert on an existing key keeps its position (like dict.update),
/// new keys are appended at the end. Any read/parse error → defaults
/// (Python logs and returns the defaults — here just the defaults).
pub fn load_settings(config_file: &Path) -> Map<String, Value> {
    let mut result = defaults();
    if let Ok(text) = std::fs::read_to_string(config_file) {
        if let Ok(Value::Object(loaded)) = serde_json::from_str::<Value>(&text) {
            for (k, v) in loaded {
                result.insert(k, v);
            }
        }
    }
    result
}

/// Serialize the settings to a string — byte for byte like `json.dump(indent=2,
/// ensure_ascii=False)`: 2-space indent, UTF-8 without escaping,
/// no trailing newline.
pub fn serialize_settings(settings: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(&Value::Object(settings.clone()))
        .expect("serde_json Map всегда сериализуется")
}

/// Settings manager — counterpart of `SettingsManager`.
pub struct SettingsManager {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub settings: Map<String, Value>,
}

impl SettingsManager {
    /// Create: `$XDG_CONFIG_HOME/rexon/settings.json` (default
    /// `~/.config/rexon`), create the directory, load.
    pub fn new() -> std::io::Result<Self> {
        let config_dir = crate::utils::rexon_config_dir();
        let config_file = config_dir.join("settings.json");
        std::fs::create_dir_all(&config_dir)?;
        let settings = load_settings(&config_file);
        Ok(Self { config_dir, config_file, settings })
    }

    /// Saving goes through the shared door — [`crate::utils::atomic_write`].
    ///
    /// ALL of the user's settings live here, and they get no second chance:
    /// a truncated `settings.json` reads as empty, i.e. everything the person
    /// has ever configured rolls back to the defaults.
    pub fn save_settings(&self) -> std::io::Result<()> {
        let data = serialize_settings(&self.settings);
        crate::utils::atomic_write(&self.config_file.to_string_lossy(), &data)
    }

    /// Get a value (None → missing).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.settings.get(key)
    }

    /// Set a value and save.
    pub fn set(&mut self, key: &str, value: Value) -> std::io::Result<()> {
        self.settings.insert(key.to_string(), value);
        self.save_settings()
    }

    /// Current file manager (auto-detected if "auto" or missing).
    pub fn get_file_manager(&self) -> String {
        match self.settings.get("file_manager").and_then(Value::as_str) {
            Some("auto") | None => detect_file_manager(&command_exists),
            Some(fm) => fm.to_string(),
        }
    }
}

/// The first installed file manager, otherwise "nautilus". The predicate
/// is a parameter — so tests do not depend on the system PATH.
pub fn detect_file_manager(exists: &dyn Fn(&str) -> bool) -> String {
    for fm in FILE_MANAGERS {
        if exists(fm) {
            return fm.to_string();
        }
    }
    "nautilus".to_string()
}

/// Whether the executable `cmd` exists in one of the PATH directories.
fn command_exists(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file())
}

/// Path to `settings.json` without creating a manager.
///
/// Needed by those who only READ the settings and must not touch the directory:
/// the wallpaper host watches this file to pick up edits live, and it has no
/// business creating the config on behalf of the settings window.
pub fn settings_file() -> PathBuf {
    crate::utils::rexon_config_dir().join("settings.json")
}
