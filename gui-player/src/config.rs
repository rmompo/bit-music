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

pub const WINDOW_MAXIMIZED: &str = "windowMaximized";
pub const WINDOW_X: &str = "windowX";
pub const WINDOW_Y: &str = "windowY";
pub const WINDOW_WIDTH: &str = "windowWidth";
pub const WINDOW_HEIGHT: &str = "windowHeight";
pub const TABS_WIDTH_PERCENT: &str = "tabsWidthPercent";
pub const TOP_HEIGHT_PERCENT: &str = "topHeightPercent";
const DEFAULT_WIDTH: i32 = 1100;
const DEFAULT_HEIGHT: i32 = 720;

/// How the window was left: maximized, or restored with a position (when
/// the system tells it) and a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    pub maximized: bool,
    pub position: Option<(i32, i32)>,
    pub width: i32,
    pub height: i32,
}

/// Where the dividers were left, as percentages: A's share of the width of
/// A + B, and the share of the height taken by A + B (the rest is C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividerState {
    pub tabs_width_percent: i32,
    pub top_height_percent: i32,
}

impl Default for DividerState {
    fn default() -> Self {
        Self { tabs_width_percent: 30, top_height_percent: 50 }
    }
}

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
    ///
    /// A setting without a `default` in the schema is optional: it is not
    /// added when missing, and an invalid value is removed.
    pub fn with_defaults(mut self) -> Self {
        for (key, spec) in setting_specs() {
            let default = spec.get("default").map(value_text);
            match (self.settings.iter_mut().find(|s| s.key == key), default) {
                (Some(s), _) if is_valid(&spec, &s.value) => {}
                (Some(s), Some(default)) => s.value = default,
                (Some(_), None) => self.settings.retain(|s| s.key != key),
                (None, Some(default)) => self.settings.push(KeyValue { key, value: default }),
                (None, None) => {}
            }
        }
        self
    }

    /// Sets a setting, adding it if it is not there yet.
    pub fn set_setting(&mut self, key: &str, value: String) {
        match self.settings.iter_mut().find(|s| s.key == key) {
            Some(s) => s.value = value,
            None => self.settings.push(KeyValue { key: key.to_string(), value }),
        }
    }

    fn remove_setting(&mut self, key: &str) {
        self.settings.retain(|s| s.key != key);
    }

    fn int_setting(&self, key: &str) -> Option<i32> {
        self.setting(key)?.parse().ok()
    }

    /// The window state stored in the settings.
    pub fn window(&self) -> WindowState {
        let position = match (self.int_setting(WINDOW_X), self.int_setting(WINDOW_Y)) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        };
        WindowState {
            maximized: self.setting(WINDOW_MAXIMIZED).is_none_or(|v| v == "true"),
            position,
            width: self.int_setting(WINDOW_WIDTH).unwrap_or(DEFAULT_WIDTH),
            height: self.int_setting(WINDOW_HEIGHT).unwrap_or(DEFAULT_HEIGHT),
        }
    }

    /// The divider positions stored in the settings.
    pub fn dividers(&self) -> DividerState {
        let d = DividerState::default();
        DividerState {
            tabs_width_percent: self.int_setting(TABS_WIDTH_PERCENT).unwrap_or(d.tabs_width_percent),
            top_height_percent: self.int_setting(TOP_HEIGHT_PERCENT).unwrap_or(d.top_height_percent),
        }
    }

    pub fn set_dividers(&mut self, d: &DividerState) {
        self.set_setting(TABS_WIDTH_PERCENT, d.tabs_width_percent.to_string());
        self.set_setting(TOP_HEIGHT_PERCENT, d.top_height_percent.to_string());
    }

    /// Stores the window state in the settings.
    pub fn set_window(&mut self, w: &WindowState) {
        self.set_setting(WINDOW_MAXIMIZED, w.maximized.to_string());
        match w.position {
            Some((x, y)) => {
                self.set_setting(WINDOW_X, x.to_string());
                self.set_setting(WINDOW_Y, y.to_string());
            }
            None => {
                self.remove_setting(WINDOW_X);
                self.remove_setting(WINDOW_Y);
            }
        }
        self.set_setting(WINDOW_WIDTH, w.width.to_string());
        self.set_setting(WINDOW_HEIGHT, w.height.to_string());
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
    fn the_window_opens_maximized_by_default_and_round_trips() {
        let mut config = Config::default().with_defaults();
        let w = config.window();
        assert!(w.maximized);
        assert_eq!((w.width, w.height, w.position), (1100, 720, None));
        // Position is optional: it is not written until known.
        assert_eq!(config.setting(WINDOW_X), None);

        let restored = WindowState { maximized: false, position: Some((-20, 40)), width: 900, height: 600 };
        config.set_window(&restored);
        assert_eq!(config.window(), restored);
        // Survives the schema check.
        assert_eq!(config.clone().with_defaults().window(), restored);
        // A too-small size is reset to the default.
        config.set_setting(WINDOW_WIDTH, "10".into());
        assert_eq!(config.with_defaults().window().width, 1100);
    }

    #[test]
    fn dividers_default_to_30_70_and_50_50_and_round_trip() {
        let mut config = Config::default().with_defaults();
        assert_eq!(config.dividers(), DividerState { tabs_width_percent: 30, top_height_percent: 50 });
        let moved = DividerState { tabs_width_percent: 45, top_height_percent: 62 };
        config.set_dividers(&moved);
        assert_eq!(config.clone().with_defaults().dividers(), moved);
        // Out of range values are reset to their default.
        config.set_setting(TABS_WIDTH_PERCENT, "99".into());
        assert_eq!(config.with_defaults().dividers().tabs_width_percent, 30);
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
