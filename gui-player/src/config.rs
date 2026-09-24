//! `gui-player.json`: settings and history, stored next to the executable.
//!
//! Its structure is described by `gui-player.schema.json` (embedded in the
//! executable), which is also where the default of every setting comes
//! from. If the file does not exist it is created with those defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The schema of the configuration file.
pub const SCHEMA: &str = include_str!("../gui-player.schema.json");
pub const CONFIG_FILE: &str = "gui-player.json";
/// Setting: maximum number of entries kept in `lastOpened`.
pub const MAX_LAST_OPENED: &str = "maxLastOpened";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub settings: Vec<KeyValue>,
    #[serde(rename = "lastOpened")]
    pub last_opened: Vec<KeyValue>,
}

/// The `x-settings` section of the schema: one entry per known setting.
fn setting_specs() -> serde_json::Map<String, Value> {
    let schema: Value = serde_json::from_str(SCHEMA).expect("the embedded schema is valid JSON");
    schema["properties"]["settings"]["x-settings"]
        .as_object()
        .cloned()
        .unwrap_or_default()
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether `text` is acceptable for a setting described by `spec`.
fn is_valid(spec: &Value, text: &str) -> bool {
    match spec["type"].as_str() {
        Some("integer") => text.parse::<i64>().is_ok_and(|n| {
            spec["minimum"].as_i64().is_none_or(|min| n >= min)
                && spec["maximum"].as_i64().is_none_or(|max| n <= max)
        }),
        Some("boolean") => text.parse::<bool>().is_ok(),
        _ => true,
    }
}

impl Config {
    /// Default location: next to the running executable.
    pub fn default_path() -> Option<PathBuf> {
        Some(std::env::current_exe().ok()?.parent()?.join(CONFIG_FILE))
    }

    /// Reads `path`, or creates it with the defaults when it does not exist.
    /// Returns a warning when the file could not be used: in that case the
    /// defaults are used in memory and the file is left untouched.
    pub fn load_or_create(path: &Path) -> (Self, Option<String>) {
        if !path.exists() {
            let config = Self::default().with_defaults();
            let warning = config
                .save(path)
                .err()
                .map(|e| format!("could not create {}: {e}", path.display()));
            return (config, warning);
        }
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<Config>(&text).map_err(|e| e.to_string()));
        match parsed {
            Ok(config) => {
                let completed = config.clone().with_defaults();
                let warning = if completed != config {
                    completed.save(path).err().map(|e| format!("could not update {}: {e}", path.display()))
                } else {
                    None
                };
                (completed, warning)
            }
            Err(e) => (
                Self::default().with_defaults(),
                Some(format!("could not read {}: {e}", path.display())),
            ),
        }
    }

    /// Adds every setting that is missing and resets the invalid ones to
    /// their default from the schema.
    pub fn with_defaults(mut self) -> Self {
        for (key, spec) in setting_specs() {
            let default = value_text(&spec["default"]);
            match self.settings.iter_mut().find(|s| s.key == key) {
                Some(s) if is_valid(&spec, &s.value) => {}
                Some(s) => s.value = default,
                None => self.settings.push(KeyValue { key, value: default }),
            }
        }
        self
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        text.push('\n');
        std::fs::write(path, text)
    }

    pub fn setting(&self, key: &str) -> Option<&str> {
        self.settings.iter().find(|s| s.key == key).map(|s| s.value.as_str())
    }

    /// The maximum size of the history (setting, or its schema default).
    pub fn max_last_opened(&self) -> usize {
        self.setting(MAX_LAST_OPENED)
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n >= 1)
            .unwrap_or(10)
    }

    /// Puts `path` first (most recent; moving it if it was already there)
    /// and drops the oldest ones beyond the maximum.
    pub fn record_opened(&mut self, path: &str) {
        self.last_opened.retain(|e| e.key != path);
        self.last_opened.insert(0, KeyValue { key: path.to_string(), value: path.to_string() });
        self.last_opened.truncate(self.max_last_opened());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gui-player-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(CONFIG_FILE)
    }

    #[test]
    fn the_embedded_schema_is_valid_and_defines_the_history_limit() {
        let specs = setting_specs();
        assert_eq!(specs[MAX_LAST_OPENED]["default"], 10);
    }

    #[test]
    fn a_missing_file_is_created_with_the_defaults() {
        let path = temp_file("create");
        let (config, warning) = Config::load_or_create(&path);
        assert!(warning.is_none());
        assert_eq!(config.setting(MAX_LAST_OPENED), Some("10"));
        assert!(config.last_opened.is_empty());
        let on_disk: Config = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk, config);
    }

    #[test]
    fn missing_or_invalid_settings_fall_back_to_their_default() {
        let path = temp_file("defaults");
        std::fs::write(&path, r#"{"settings":[{"key":"maxLastOpened","value":"zero"},{"key":"other","value":"x"}]}"#).unwrap();
        let (config, _) = Config::load_or_create(&path);
        assert_eq!(config.setting(MAX_LAST_OPENED), Some("10"));
        // Unknown settings are kept.
        assert_eq!(config.setting("other"), Some("x"));
    }

    #[test]
    fn a_corrupt_file_is_reported_and_left_untouched() {
        let path = temp_file("corrupt");
        std::fs::write(&path, "{ nope").unwrap();
        let (config, warning) = Config::load_or_create(&path);
        assert!(warning.unwrap().contains("could not read"));
        assert_eq!(config.max_last_opened(), 10);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ nope");
    }

    #[test]
    fn history_is_newest_first_up_to_the_limit_without_duplicates() {
        let mut config = Config::default().with_defaults();
        config.settings.iter_mut().find(|s| s.key == MAX_LAST_OPENED).unwrap().value = "3".into();
        for p in ["a", "b", "c", "a", "d"] {
            config.record_opened(p);
        }
        let keys: Vec<&str> = config.last_opened.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["d", "a", "c"]); // newest first
        assert!(config.last_opened.iter().all(|e| e.key == e.value));
    }
}
